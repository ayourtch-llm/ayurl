use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures::io::AsyncRead;

use crate::error::{AyurlError, Result};

/// A streaming response from a get or put operation.
///
/// `Response` implements `futures::io::AsyncRead`, making it composable
/// with the standard async I/O ecosystem. For convenience, it also
/// provides one-shot consumers like `bytes()`, `text()`, and their
/// lossy (error-swallowing) variants.
pub struct Response {
    inner: Box<dyn AsyncRead + Send + Unpin>,
    content_length: Option<u64>,
}

impl std::fmt::Debug for Response {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Response")
            .field("content_length", &self.content_length)
            .finish_non_exhaustive()
    }
}

impl Response {
    pub fn new(
        reader: Box<dyn AsyncRead + Send + Unpin>,
        content_length: Option<u64>,
    ) -> Self {
        Self {
            inner: reader,
            content_length,
        }
    }

    /// The content length, if known ahead of time.
    pub fn content_length(&self) -> Option<u64> {
        self.content_length
    }

    /// Consume the response, reading all bytes into a `Vec<u8>`.
    pub async fn bytes(self) -> Result<Vec<u8>> {
        use futures::io::AsyncReadExt;
        let mut buf = match self.content_length {
            Some(len) => Vec::with_capacity(len as usize),
            None => Vec::new(),
        };
        let mut reader = self.inner;
        reader.read_to_end(&mut buf).await?;
        Ok(buf)
    }

    /// Consume the response, reading all bytes and decoding as UTF-8.
    pub async fn text(self) -> Result<String> {
        let bytes = self.bytes().await?;
        String::from_utf8(bytes).map_err(|e| AyurlError::Handler(Box::new(e)))
    }

    /// Consume the response, reading all bytes. Returns what was received
    /// on error (never fails).
    pub async fn bytes_lossy(self) -> Vec<u8> {
        use futures::io::AsyncReadExt;
        let mut buf = match self.content_length {
            Some(len) => Vec::with_capacity(len as usize),
            None => Vec::new(),
        };
        let mut reader = self.inner;
        let _ = reader.read_to_end(&mut buf).await;
        buf
    }

    /// Consume the response as a string. Returns what was received on
    /// error, with invalid UTF-8 replaced by U+FFFD (never fails).
    pub async fn text_lossy(self) -> String {
        let bytes = self.bytes_lossy().await;
        String::from_utf8_lossy(&bytes).into_owned()
    }

    /// Return a reader that propagates errors normally.
    pub fn reader(self) -> impl AsyncRead + Send + Unpin {
        self.inner
    }

    /// Return a reader that swallows errors and returns EOF instead.
    pub fn lenient_reader(self) -> LenientReader {
        LenientReader {
            inner: self.inner,
            errored: false,
        }
    }
}

impl AsyncRead for Response {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

/// A reader that returns EOF on error instead of propagating it.
pub struct LenientReader {
    inner: Box<dyn AsyncRead + Send + Unpin>,
    errored: bool,
}

impl AsyncRead for LenientReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        if self.errored {
            return Poll::Ready(Ok(0)); // EOF
        }
        match Pin::new(&mut self.inner).poll_read(cx, buf) {
            Poll::Ready(Err(_)) => {
                self.errored = true;
                tracing::debug!("lenient reader: swallowed error, returning EOF");
                Poll::Ready(Ok(0))
            }
            other => other,
        }
    }
}
