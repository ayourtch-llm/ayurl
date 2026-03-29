use async_trait::async_trait;
use futures::io::AsyncRead;
use tokio_util::compat::TokioAsyncReadCompatExt;

use crate::error::Result;
use crate::scheme::{SchemeCapabilities, SchemeHandler, TransferContext};
use crate::uri::ParsedUri;

/// Handler for `file://` URIs — reads and writes local files.
pub struct FileHandler;

impl FileHandler {
    fn url_to_path(uri: &ParsedUri) -> Result<std::path::PathBuf> {
        Ok(std::path::PathBuf::from(uri.path()))
    }
}

#[async_trait]
impl SchemeHandler for FileHandler {
    async fn get(
        &self,
        uri: &ParsedUri,
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
        uri: &ParsedUri,
        mut body: Box<dyn AsyncRead + Send + Unpin>,
        _ctx: &mut TransferContext,
    ) -> Result<u64> {
        let path = Self::url_to_path(uri)?;
        tracing::debug!(?path, "file handler: opening for write (atomic via temp+rename)");

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        // Write to a temporary file in the same directory, then atomically rename.
        // Same directory ensures rename() doesn't cross filesystem boundaries.
        let tmp_path = {
            let mut tmp = path.as_os_str().to_os_string();
            tmp.push(format!(".tmp.{}", std::process::id()));
            std::path::PathBuf::from(tmp)
        };

        let bytes_written = {
            let file = tokio::fs::File::create(&tmp_path).await?;
            let mut compat_file = TokioAsyncWriteCompatExt::compat_write(file);
            futures::io::copy(&mut body, &mut compat_file).await?
        };

        // Atomic rename over the target
        if let Err(e) = tokio::fs::rename(&tmp_path, &path).await {
            // Clean up the temp file on rename failure
            let _ = tokio::fs::remove_file(&tmp_path).await;
            return Err(e.into());
        }

        tracing::debug!(?path, bytes_written, "file handler: write complete");
        Ok(bytes_written)
    }

    async fn content_length(&self, uri: &ParsedUri) -> Result<Option<u64>> {
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
