use async_trait::async_trait;
use futures::io::AsyncRead;
use url::Url;

use crate::error::{AyurlError, Result};
use crate::scheme::{SchemeCapabilities, SchemeHandler, TransferContext};

use super::ssh_common::{parse_ssh_url, request_ssh_credentials, SshOptions};

/// Handler for `sftp://` URIs using ayssh.
///
/// URL format: `sftp://[user[:password]@]host[:port]/path`
///
/// Supports password auth (from URL or credential callback) and
/// public key auth (via `SshOptions`).
pub struct SftpHandler;

/// Default read chunk size for SFTP reads (32KB).
const SFTP_READ_CHUNK: u32 = 32768;

#[async_trait]
impl SchemeHandler for SftpHandler {
    async fn get(
        &self,
        uri: &Url,
        ctx: &mut TransferContext,
    ) -> Result<Box<dyn AsyncRead + Send + Unpin>> {
        let target = parse_ssh_url(uri)?;
        tracing::debug!(
            host = %target.host,
            port = target.port,
            user = %target.username,
            path = %target.path,
            "sftp handler: GET (download)"
        );

        let mut client = connect_sftp(uri, ctx).await?;

        // Open the remote file for reading
        let handle = client
            .open(
                &target.path,
                ayssh::sftp::sftp_flags::SSH_FXF_READ,
                &ayssh::sftp::SftpAttrs::default(),
            )
            .await
            .map_err(|e| AyurlError::Connection(format!("SFTP open failed: {e}")))?;

        // Read the file in chunks
        let mut data = Vec::new();
        let mut offset = 0u64;
        loop {
            let chunk = client
                .read(&handle, offset, SFTP_READ_CHUNK)
                .await;

            match chunk {
                Ok(bytes) if bytes.is_empty() => break,
                Ok(bytes) => {
                    offset += bytes.len() as u64;
                    data.extend_from_slice(&bytes);
                }
                Err(_) if !data.is_empty() => {
                    // Got an error after reading some data — likely EOF
                    break;
                }
                Err(e) => {
                    return Err(AyurlError::Connection(format!("SFTP read failed: {e}")));
                }
            }
        }

        let _ = client.close(&handle).await;
        let _ = client.disconnect().await;

        tracing::debug!(bytes = data.len(), "sftp handler: download complete");
        Ok(Box::new(futures::io::Cursor::new(data)))
    }

    async fn put(
        &self,
        uri: &Url,
        mut body: Box<dyn AsyncRead + Send + Unpin>,
        ctx: &mut TransferContext,
    ) -> Result<u64> {
        let target = parse_ssh_url(uri)?;
        tracing::debug!(
            host = %target.host,
            port = target.port,
            user = %target.username,
            path = %target.path,
            "sftp handler: PUT (upload)"
        );

        // Read body into memory
        let mut data = Vec::new();
        futures::io::AsyncReadExt::read_to_end(&mut body, &mut data).await?;
        let len = data.len() as u64;

        let mut client = connect_sftp(uri, ctx).await?;

        // Open (create/truncate) the remote file for writing
        let handle = client
            .open(
                &target.path,
                ayssh::sftp::sftp_flags::SSH_FXF_WRITE
                    | ayssh::sftp::sftp_flags::SSH_FXF_CREAT
                    | ayssh::sftp::sftp_flags::SSH_FXF_TRUNC,
                &ayssh::sftp::SftpAttrs::default(),
            )
            .await
            .map_err(|e| AyurlError::Connection(format!("SFTP open for write failed: {e}")))?;

        // Write in chunks
        let mut offset = 0u64;
        for chunk in data.chunks(SFTP_READ_CHUNK as usize) {
            client
                .write(&handle, offset, chunk)
                .await
                .map_err(|e| AyurlError::Connection(format!("SFTP write failed: {e}")))?;
            offset += chunk.len() as u64;
        }

        let _ = client.close(&handle).await;
        let _ = client.disconnect().await;

        tracing::debug!(bytes = len, "sftp handler: upload complete");
        Ok(len)
    }

    fn capabilities(&self) -> SchemeCapabilities {
        SchemeCapabilities {
            supports_streaming: false, // buffered via Vec<u8> for now
            supports_seek: false,
            supports_content_length: false,
        }
    }
}

/// Establish an SFTP connection using the best available auth method.
///
/// Priority: private key (from SshOptions) → password from URL → credential callback.
async fn connect_sftp(
    uri: &Url,
    ctx: &TransferContext,
) -> Result<ayssh::sftp::SftpClient> {
    let target = parse_ssh_url(uri)?;

    // Check for private key in options
    let ssh_opts = ctx.options::<SshOptions>();
    let private_key = match ssh_opts {
        Some(opts) => opts.load_private_key().await?,
        None => None,
    };

    if let Some(key) = private_key {
        return ayssh::sftp::SftpClient::connect_with_publickey(
            &target.host,
            target.port,
            &target.username,
            &key,
        )
        .await
        .map_err(|e| AyurlError::Connection(format!("SFTP connect (publickey) failed: {e}")));
    }

    if let Some(ref password) = target.password {
        return ayssh::sftp::SftpClient::connect_with_password(
            &target.host,
            target.port,
            &target.username,
            password,
        )
        .await
        .map_err(|e| AyurlError::Connection(format!("SFTP connect (password) failed: {e}")));
    }

    // No credentials in URL or options — request via callback
    let creds = request_ssh_credentials(uri, &target, ctx)?;
    let password = creds.secret.unwrap_or_default();
    let username = creds.username.unwrap_or(target.username);

    ayssh::sftp::SftpClient::connect_with_password(
        &target.host,
        target.port,
        &username,
        &password,
    )
    .await
    .map_err(|e| AyurlError::Connection(format!("SFTP connect (password) failed: {e}")))
}
