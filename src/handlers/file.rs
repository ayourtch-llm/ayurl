use async_trait::async_trait;
use futures::io::AsyncRead;
use tokio_util::compat::TokioAsyncReadCompatExt;
use url::Url;

use crate::error::{AyurlError, Result};
use crate::scheme::{SchemeCapabilities, SchemeHandler, TransferContext};

/// Handler for `file://` URIs — reads and writes local files.
pub struct FileHandler;

impl FileHandler {
    fn url_to_path(uri: &Url) -> Result<std::path::PathBuf> {
        uri.to_file_path()
            .map_err(|_| AyurlError::InvalidUri(format!("not a valid file path: {uri}")))
    }
}

#[async_trait]
impl SchemeHandler for FileHandler {
    async fn get(
        &self,
        uri: &Url,
        _ctx: &mut TransferContext,
    ) -> Result<Box<dyn AsyncRead + Send + Unpin>> {
        let path = Self::url_to_path(uri)?;
        tracing::debug!(?path, "file handler: opening for read");
        let file = tokio::fs::File::open(&path).await?;
        // Convert tokio::fs::File (tokio AsyncRead) to futures AsyncRead via compat
        Ok(Box::new(file.compat()))
    }

    async fn put(
        &self,
        uri: &Url,
        mut body: Box<dyn AsyncRead + Send + Unpin>,
        _ctx: &mut TransferContext,
    ) -> Result<u64> {
        let path = Self::url_to_path(uri)?;
        tracing::debug!(?path, "file handler: opening for write");

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let file = tokio::fs::File::create(&path).await?;
        let mut compat_file = TokioAsyncWriteCompatExt::compat_write(file);

        let bytes_written = futures::io::copy(&mut body, &mut compat_file).await?;
        tracing::debug!(?path, bytes_written, "file handler: write complete");
        Ok(bytes_written)
    }

    async fn content_length(&self, uri: &Url) -> Result<Option<u64>> {
        let path = Self::url_to_path(uri)?;
        match tokio::fs::metadata(&path).await {
            Ok(meta) => Ok(Some(meta.len())),
            Err(_) => Ok(None),
        }
    }

    fn capabilities(&self) -> SchemeCapabilities {
        SchemeCapabilities {
            supports_streaming: true,
            supports_seek: true,
            supports_content_length: true,
        }
    }
}

use tokio_util::compat::TokioAsyncWriteCompatExt;
