use std::path::PathBuf;

use url::Url;

use crate::error::{AyurlError, Result};
use crate::scheme::{CredentialKind, CredentialRequest, Credentials, TransferContext};

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
pub fn parse_ssh_url(uri: &Url) -> Result<SshTarget> {
    let host = uri
        .host_str()
        .ok_or_else(|| AyurlError::InvalidUri(format!("missing host in {uri}")))?
        .to_string();

    let port = uri.port().unwrap_or(22);

    let username = {
        let u = uri.username();
        if u.is_empty() {
            // Try current user
            std::env::var("USER")
                .or_else(|_| std::env::var("USERNAME"))
                .unwrap_or_else(|_| "root".to_string())
        } else {
            u.to_string()
        }
    };

    let password = uri.password().map(|p| p.to_string());

    // URL path: strip leading slash for absolute paths on remote
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
    uri: &Url,
    target: &SshTarget,
    ctx: &TransferContext,
) -> Result<Credentials> {
    let host = &target.host;
    let cred_req = CredentialRequest {
        url: uri.clone(),
        scheme: uri.scheme().to_string(),
        kind: CredentialKind::UsernamePassword,
        message: format!("Authentication required for {host}"),
        prompts: Vec::new(),
    };

    ctx.request_credentials(&cred_req)
        .ok_or_else(|| AyurlError::Connection(format!("no credentials provided for {host}")))
}
