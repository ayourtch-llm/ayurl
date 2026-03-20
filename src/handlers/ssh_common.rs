use std::path::PathBuf;

use crate::error::{AyurlError, Result};
use crate::scheme::{CredentialKind, CredentialRequest, Credentials, TransferContext};
use crate::uri::ParsedUri;

/// Scheme-specific options for SCP and SFTP transfers.
#[derive(Debug, Clone, Default)]
pub struct SshOptions {
    /// PEM or OpenSSH private key bytes (in-memory).
    pub private_key: Option<Vec<u8>>,
    /// Path to a private key file (loaded on demand).
    pub private_key_path: Option<PathBuf>,
    /// File mode for uploads (default: 0o644).
    pub file_mode: Option<u32>,
}

impl SshOptions {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the private key from in-memory bytes.
    pub fn with_private_key(mut self, key: impl Into<Vec<u8>>) -> Self {
        self.private_key = Some(key.into());
        self
    }

    /// Set the path to a private key file.
    pub fn with_private_key_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.private_key_path = Some(path.into());
        self
    }

    /// Set the file mode for uploads.
    pub fn with_file_mode(mut self, mode: u32) -> Self {
        self.file_mode = Some(mode);
        self
    }

    /// Load the private key bytes, either from `private_key` or by reading `private_key_path`.
    pub async fn load_private_key(&self) -> Result<Option<Vec<u8>>> {
        if let Some(ref key) = self.private_key {
            return Ok(Some(key.clone()));
        }
        if let Some(ref path) = self.private_key_path {
            let key = tokio::fs::read(path).await.map_err(|e| {
                AyurlError::Connection(format!(
                    "failed to read private key from {}: {e}",
                    path.display()
                ))
            })?;
            return Ok(Some(key));
        }
        Ok(None)
    }
}

/// Parsed SSH connection parameters from a URL.
pub struct SshTarget {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: Option<String>,
    pub path: String,
}

/// Parse an SCP/SFTP URL into connection parameters.
/// Format: scp://user:pass@host:port/path or sftp://user@host/path
pub fn parse_ssh_url(uri: &ParsedUri) -> Result<SshTarget> {
    let host = uri
        .host()
        .ok_or_else(|| AyurlError::InvalidUri(format!("missing host in {uri}")))?
        .to_string();

    let port = uri.port().unwrap_or(22);

    let username = match uri.username() {
        Some(u) => u.to_string(),
        None => {
            // Try current user
            std::env::var("USER")
                .or_else(|_| std::env::var("USERNAME"))
                .unwrap_or_else(|_| "root".to_string())
        }
    };

    let password = uri.password().map(|p| p.to_string());

    // URL path: strip leading slash for relative paths on remote
    let path = uri.path().to_string();
    let path = if path.starts_with('/') {
        path[1..].to_string()
    } else {
        path
    };

    if path.is_empty() {
        return Err(AyurlError::InvalidUri(format!(
            "missing remote path in {uri}"
        )));
    }

    Ok(SshTarget {
        host,
        port,
        username,
        password,
        path,
    })
}

/// Request credentials via the callback if password is not available.
pub fn request_ssh_credentials(
    uri: &ParsedUri,
    target: &SshTarget,
    ctx: &TransferContext,
) -> Result<Credentials> {
    let host = &target.host;
    let cred_req = CredentialRequest {
        uri: uri.clone(),
        scheme: uri.scheme().to_string(),
        kind: CredentialKind::UsernamePassword,
        message: format!("Authentication required for {host}"),
        prompts: Vec::new(),
    };

    ctx.request_credentials(&cred_req)
        .ok_or_else(|| AyurlError::Connection(format!("no credentials provided for {host}")))
}

/// Spawn an `SshChannelReader` into a background task and return a
/// `futures::io::AsyncRead` that receives chunks through a channel.
///
/// This avoids the borrow-checker issue of polling `read_chunk()` inside
/// `poll_read()`, and provides true streaming with backpressure.
pub fn channel_reader_to_async_read(
    mut reader: ayssh::sftp::SshChannelReader,
) -> (impl futures::io::AsyncRead + Send + Unpin, u64) {
    let content_length = reader.content_length();

    // Bounded channel provides backpressure — producer waits if consumer is slow
    let (tx, rx) = tokio::sync::mpsc::channel::<std::result::Result<Vec<u8>, std::io::Error>>(4);

    tokio::spawn(async move {
        loop {
            match reader.read_chunk().await {
                Ok(chunk) if chunk.is_empty() => break,
                Ok(chunk) => {
                    if tx.send(Ok(chunk)).await.is_err() {
                        break; // receiver dropped
                    }
                }
                Err(e) => {
                    let _ = tx
                        .send(Err(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            e.to_string(),
                        )))
                        .await;
                    break;
                }
            }
        }
    });

    let reader = ChannelStreamReader {
        rx,
        buf: Vec::new(),
        buf_pos: 0,
        done: false,
    };

    (reader, content_length)
}

/// Async reader that pulls chunks from a background task via mpsc channel.
pub struct ChannelStreamReader {
    rx: tokio::sync::mpsc::Receiver<std::result::Result<Vec<u8>, std::io::Error>>,
    buf: Vec<u8>,
    buf_pos: usize,
    done: bool,
}

impl futures::io::AsyncRead for ChannelStreamReader {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        // Serve buffered data first
        if self.buf_pos < self.buf.len() {
            let available = &self.buf[self.buf_pos..];
            let to_copy = available.len().min(buf.len());
            buf[..to_copy].copy_from_slice(&available[..to_copy]);
            self.buf_pos += to_copy;
            return std::task::Poll::Ready(Ok(to_copy));
        }

        if self.done {
            return std::task::Poll::Ready(Ok(0));
        }

        // Poll the channel for the next chunk
        match self.rx.poll_recv(cx) {
            std::task::Poll::Ready(Some(Ok(chunk))) => {
                if chunk.is_empty() {
                    self.done = true;
                    return std::task::Poll::Ready(Ok(0));
                }
                let to_copy = chunk.len().min(buf.len());
                buf[..to_copy].copy_from_slice(&chunk[..to_copy]);
                if to_copy < chunk.len() {
                    self.buf = chunk;
                    self.buf_pos = to_copy;
                } else {
                    self.buf.clear();
                    self.buf_pos = 0;
                }
                std::task::Poll::Ready(Ok(to_copy))
            }
            std::task::Poll::Ready(Some(Err(e))) => {
                self.done = true;
                std::task::Poll::Ready(Err(e))
            }
            std::task::Poll::Ready(None) => {
                // Channel closed = EOF
                self.done = true;
                std::task::Poll::Ready(Ok(0))
            }
            std::task::Poll::Pending => std::task::Poll::Pending,
        }
    }
}

/// Convert a `futures::io::AsyncRead` into a `tokio::io::AsyncRead`
/// for ayssh's upload methods which expect tokio's trait.
pub struct FuturesToTokioReader<R> {
    inner: R,
}

impl<R> FuturesToTokioReader<R> {
    pub fn new(inner: R) -> Self {
        Self { inner }
    }
}

impl<R: futures::io::AsyncRead + Unpin> tokio::io::AsyncRead for FuturesToTokioReader<R> {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let slice = buf.initialize_unfilled();
        match std::pin::Pin::new(&mut self.inner).poll_read(cx, slice) {
            std::task::Poll::Ready(Ok(n)) => {
                buf.advance(n);
                std::task::Poll::Ready(Ok(()))
            }
            std::task::Poll::Ready(Err(e)) => std::task::Poll::Ready(Err(e)),
            std::task::Poll::Pending => std::task::Poll::Pending,
        }
    }
}
