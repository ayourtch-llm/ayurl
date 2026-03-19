use std::any::Any;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures::io::{AsyncRead, AsyncWrite};
use url::Url;

use crate::error::Result;
use crate::progress::ProgressSink;

/// Trait for providing network connections (transport-level abstraction).
///
/// Scheme handlers use a `Connector` to establish byte streams rather than
/// connecting directly. This enables transparent tunneling through SSH,
/// SOCKS proxies, etc.
#[async_trait]
pub trait Connector: Send + Sync {
    async fn connect(
        &self,
        host: &str,
        port: u16,
    ) -> Result<Box<dyn AsyncReadWrite + Send + Unpin>>;
}

/// Combined AsyncRead + AsyncWrite trait for bidirectional byte streams.
pub trait AsyncReadWrite: AsyncRead + AsyncWrite {}
impl<T: AsyncRead + AsyncWrite> AsyncReadWrite for T {}

/// Default connector that establishes direct TCP connections.
pub struct DirectConnector;

#[async_trait]
impl Connector for DirectConnector {
    async fn connect(
        &self,
        host: &str,
        port: u16,
    ) -> Result<Box<dyn AsyncReadWrite + Send + Unpin>> {
        use tokio::net::TcpStream;
        let stream = TcpStream::connect((host, port)).await?;
        // Convert tokio TcpStream to futures-compatible via Compat
        let compat = tokio_util::compat::TokioAsyncReadCompatExt::compat(stream);
        Ok(Box::new(compat))
    }
}

/// Describes what a scheme handler is capable of.
#[derive(Debug, Clone)]
pub struct SchemeCapabilities {
    pub supports_streaming: bool,
    pub supports_seek: bool,
    pub supports_content_length: bool,
}

impl Default for SchemeCapabilities {
    fn default() -> Self {
        Self {
            supports_streaming: true,
            supports_seek: false,
            supports_content_length: false,
        }
    }
}

/// Context passed to scheme handlers during transfers.
///
/// Carries the connector, progress reporting, timeout, and any
/// scheme-specific options (as type-erased `Box<dyn Any>`).
pub struct TransferContext {
    pub connector: Arc<dyn Connector>,
    pub progress_sink: Option<ProgressSink>,
    pub timeout: Option<Duration>,
    pub options: Option<Box<dyn Any + Send + Sync>>,
}

impl TransferContext {
    pub fn new(connector: Arc<dyn Connector>) -> Self {
        Self {
            connector,
            progress_sink: None,
            timeout: None,
            options: None,
        }
    }

    /// Downcast scheme-specific options to a concrete type.
    pub fn options<T: 'static>(&self) -> Option<&T> {
        self.options.as_ref()?.downcast_ref::<T>()
    }
}

/// Trait that scheme handlers must implement.
///
/// Registration is at runtime but the trait bound provides compile-time
/// type safety. Every handler must support streaming get and put.
#[async_trait]
pub trait SchemeHandler: Send + Sync {
    /// Initiate a GET, returning a streaming response body.
    async fn get(
        &self,
        uri: &Url,
        ctx: &mut TransferContext,
    ) -> Result<Box<dyn AsyncRead + Send + Unpin>>;

    /// Initiate a PUT, consuming a streaming request body.
    async fn put(
        &self,
        uri: &Url,
        body: Box<dyn AsyncRead + Send + Unpin>,
        ctx: &mut TransferContext,
    ) -> Result<u64>;

    /// Optionally report the expected content length for a URI.
    async fn content_length(&self, _uri: &Url) -> Result<Option<u64>> {
        Ok(None)
    }

    /// Report this handler's capabilities.
    fn capabilities(&self) -> SchemeCapabilities {
        SchemeCapabilities::default()
    }
}
