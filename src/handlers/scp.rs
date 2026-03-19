use async_trait::async_trait;
use futures::io::AsyncRead;
use url::Url;

use crate::error::{AyurlError, Result};
use crate::scheme::{SchemeCapabilities, SchemeHandler, TransferContext};

use super::ssh_common::{
    channel_reader_to_async_read, parse_ssh_url, request_ssh_credentials, FuturesToTokioReader,
    SshOptions,
};

/// Handler for `scp://` URIs using ayssh.
///
/// URL format: `scp://[user[:password]@]host[:port]/path`
///
/// Supports password auth (from URL or credential callback) and
/// public key auth (via `SshOptions`). Uses streaming API for
/// constant-memory transfers.
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
            "scp handler: GET (download stream)"
        );

        // Check for private key in options
        let ssh_opts = ctx.options::<SshOptions>();
        let private_key = match ssh_opts {
            Some(opts) => opts.load_private_key().await?,
            None => None,
        };

        let (channel_reader, _filename, _size) = if let Some(key) = private_key {
            ayssh::sftp::ScpSession::download_stream_with_publickey(
                &target.host,
                target.port,
                &target.username,
                &key,
                &target.path,
            )
            .await
            .map_err(|e| AyurlError::Connection(format!("SCP download failed: {e}")))?
        } else if let Some(ref password) = target.password {
            ayssh::sftp::ScpSession::download_stream(
                &target.host,
                target.port,
                &target.username,
                password,
                &target.path,
            )
            .await
            .map_err(|e| AyurlError::Connection(format!("SCP download failed: {e}")))?
        } else {
            let creds = request_ssh_credentials(uri, &target, ctx)?;
            let password = creds.secret.unwrap_or_default();

            ayssh::sftp::ScpSession::download_stream(
                &target.host,
                target.port,
                creds.username.as_deref().unwrap_or(&target.username),
                &password,
                &target.path,
            )
            .await
            .map_err(|e| AyurlError::Connection(format!("SCP download failed: {e}")))?
        };

        let (reader, content_length) = channel_reader_to_async_read(channel_reader);
        tracing::debug!(
            content_length,
            "scp handler: streaming download started"
        );

        Ok(Box::new(reader))
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
            "scp handler: PUT (upload stream)"
        );

        let ssh_opts = ctx.options::<SshOptions>();
        let private_key = match ssh_opts {
            Some(opts) => opts.load_private_key().await?,
            None => None,
        };
        let file_mode = ssh_opts.and_then(|o| o.file_mode).unwrap_or(0o644);

        // SCP protocol requires file_size upfront. If content_length_hint
        // is available, we can stream directly. Otherwise, buffer first.
        let (mut tokio_reader, file_size): (Box<dyn tokio::io::AsyncRead + Send + Unpin>, u64) =
            if let Some(len) = ctx.content_length_hint {
                tracing::debug!(content_length = len, "scp upload: streaming with known size");
                (Box::new(FuturesToTokioReader::new(body)), len)
            } else {
                tracing::debug!("scp upload: buffering (no content_length_hint)");
                let mut data = Vec::new();
                let mut body = body;
                futures::io::AsyncReadExt::read_to_end(&mut body, &mut data).await?;
                let len = data.len() as u64;
                (
                    Box::new(FuturesToTokioReader::new(futures::io::Cursor::new(data))),
                    len,
                )
            };

        let bytes_written = if let Some(key) = private_key {
            ayssh::sftp::ScpSession::upload_stream_with_publickey(
                &target.host,
                target.port,
                &target.username,
                &key,
                &target.path,
                &mut *tokio_reader,
                file_size,
                file_mode,
            )
            .await
            .map_err(|e| AyurlError::Connection(format!("SCP upload failed: {e}")))?
        } else if let Some(ref password) = target.password {
            ayssh::sftp::ScpSession::upload_stream(
                &target.host,
                target.port,
                &target.username,
                password,
                &target.path,
                &mut *tokio_reader,
                file_size,
                file_mode,
            )
            .await
            .map_err(|e| AyurlError::Connection(format!("SCP upload failed: {e}")))?
        } else {
            let creds = request_ssh_credentials(uri, &target, ctx)?;
            let password = creds.secret.unwrap_or_default();

            ayssh::sftp::ScpSession::upload_stream(
                &target.host,
                target.port,
                creds.username.as_deref().unwrap_or(&target.username),
                &password,
                &target.path,
                &mut *tokio_reader,
                file_size,
                file_mode,
            )
            .await
            .map_err(|e| AyurlError::Connection(format!("SCP upload failed: {e}")))?
        };

        tracing::debug!(bytes = bytes_written, "scp handler: upload complete");
        Ok(bytes_written)
    }

    async fn content_length(&self, _uri: &Url) -> Result<Option<u64>> {
        // Could be obtained from download_stream's return tuple,
        // but that would require a full connection. Return None.
        Ok(None)
    }

    fn capabilities(&self) -> SchemeCapabilities {
        SchemeCapabilities {
            supports_streaming: true,
            supports_seek: false,
            supports_content_length: false,
        }
    }
}
