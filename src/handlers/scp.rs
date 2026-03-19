use async_trait::async_trait;
use futures::io::AsyncRead;
use url::Url;

use crate::error::{AyurlError, Result};
use crate::scheme::{SchemeCapabilities, SchemeHandler, TransferContext};

use super::ssh_common::{parse_ssh_url, request_ssh_credentials, SshOptions};

/// Handler for `scp://` URIs using ayssh.
///
/// URL format: `scp://[user[:password]@]host[:port]/path`
///
/// Supports password auth (from URL or credential callback) and
/// public key auth (via `SshOptions`).
pub struct ScpHandler;

#[async_trait]
impl SchemeHandler for ScpHandler {
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
            "scp handler: GET (download)"
        );

        // Check for private key in options
        let ssh_opts = ctx.options::<SshOptions>();
        let private_key = match ssh_opts {
            Some(opts) => opts.load_private_key().await?,
            None => None,
        };

        let data = if let Some(key) = private_key {
            // Public key auth
            ayssh::sftp::ScpSession::download_with_publickey(
                &target.host,
                target.port,
                &target.username,
                &key,
                &target.path,
            )
            .await
            .map_err(|e| AyurlError::Connection(format!("SCP download failed: {e}")))?
        } else if let Some(ref password) = target.password {
            // Password from URL
            ayssh::sftp::ScpSession::download(
                &target.host,
                target.port,
                &target.username,
                password,
                &target.path,
            )
            .await
            .map_err(|e| AyurlError::Connection(format!("SCP download failed: {e}")))?
        } else {
            // Try without password first — if ayssh requires one, request via callback
            let creds = request_ssh_credentials(uri, &target, ctx)?;
            let password = creds.secret.unwrap_or_default();

            ayssh::sftp::ScpSession::download(
                &target.host,
                target.port,
                &creds.username.as_deref().unwrap_or(&target.username),
                &password,
                &target.path,
            )
            .await
            .map_err(|e| AyurlError::Connection(format!("SCP download failed: {e}")))?
        };

        tracing::debug!(bytes = data.len(), "scp handler: download complete");
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
            "scp handler: PUT (upload)"
        );

        // Read body into memory (ayssh takes &[u8])
        let mut data = Vec::new();
        futures::io::AsyncReadExt::read_to_end(&mut body, &mut data).await?;
        let len = data.len() as u64;

        let ssh_opts = ctx.options::<SshOptions>();
        let private_key = match ssh_opts {
            Some(opts) => opts.load_private_key().await?,
            None => None,
        };
        let file_mode = ssh_opts
            .and_then(|o| o.file_mode)
            .unwrap_or(0o644);

        if let Some(key) = private_key {
            ayssh::sftp::ScpSession::upload_with_publickey(
                &target.host,
                target.port,
                &target.username,
                &key,
                &target.path,
                &data,
                file_mode,
            )
            .await
            .map_err(|e| AyurlError::Connection(format!("SCP upload failed: {e}")))?;
        } else if let Some(ref password) = target.password {
            ayssh::sftp::ScpSession::upload(
                &target.host,
                target.port,
                &target.username,
                password,
                &target.path,
                &data,
                file_mode,
            )
            .await
            .map_err(|e| AyurlError::Connection(format!("SCP upload failed: {e}")))?;
        } else {
            let creds = request_ssh_credentials(uri, &target, ctx)?;
            let password = creds.secret.unwrap_or_default();

            ayssh::sftp::ScpSession::upload(
                &target.host,
                target.port,
                &creds.username.as_deref().unwrap_or(&target.username),
                &password,
                &target.path,
                &data,
                file_mode,
            )
            .await
            .map_err(|e| AyurlError::Connection(format!("SCP upload failed: {e}")))?;
        }

        tracing::debug!(bytes = len, "scp handler: upload complete");
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
