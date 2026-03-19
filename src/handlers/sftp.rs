use async_trait::async_trait;
use futures::io::AsyncRead;
use url::Url;

use crate::error::{AyurlError, Result};
use crate::scheme::{SchemeCapabilities, SchemeHandler, TransferContext};

use super::ssh_common::{parse_ssh_url, request_ssh_credentials, FuturesToTokioReader, SshOptions};

/// Handler for `sftp://` URIs using ayssh.
///
/// URL format: `sftp://[user[:password]@]host[:port]/path`
///
/// Supports password auth (from URL or credential callback) and
/// public key auth (via `SshOptions`).
pub struct SftpHandler;

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

        // Use the high-level read_file convenience method
        let data = client
            .read_file(&target.path)
            .await
            .map_err(|e| AyurlError::Connection(format!("SFTP read failed: {e}")))?;

        let _ = client.disconnect().await;

        tracing::debug!(bytes = data.len(), "sftp handler: download complete");
        Ok(Box::new(futures::io::Cursor::new(data)))
    }

    async fn put(
        &self,
        uri: &Url,
        body: Box<dyn AsyncRead + Send + Unpin>,
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

        let ssh_opts = ctx.options::<SshOptions>();
        let file_mode = ssh_opts.and_then(|o| o.file_mode).unwrap_or(0o644);

        let mut client = connect_sftp(uri, ctx).await?;

        // Use streaming upload — wrap futures::io::AsyncRead → tokio::io::AsyncRead
        let mut tokio_reader = FuturesToTokioReader::new(body);
        let bytes_written = client
            .write_file_stream(&target.path, &mut tokio_reader, file_mode)
            .await
            .map_err(|e| AyurlError::Connection(format!("SFTP write failed: {e}")))?;

        let _ = client.disconnect().await;

        tracing::debug!(bytes = bytes_written, "sftp handler: upload complete");
        Ok(bytes_written)
    }

    fn capabilities(&self) -> SchemeCapabilities {
        SchemeCapabilities {
            supports_streaming: true,
            supports_seek: false,
            supports_content_length: false,
        }
    }
}

/// Establish an SFTP connection using the best available auth method.
///
/// Priority: private key (from SshOptions) → password from URL → credential callback.
async fn connect_sftp(uri: &Url, ctx: &TransferContext) -> Result<ayssh::sftp::SftpClient> {
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
