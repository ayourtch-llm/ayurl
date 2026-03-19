use std::any::Any;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures::io::{AsyncRead, AsyncWrite};
use url::Url;

use crate::error::Result;
use crate::progress::ProgressSink;

// --- Credential types ---

/// What kind of credential a handler is requesting.
#[derive(Debug, Clone)]
pub enum CredentialKind {
    UsernamePassword,
    BearerToken,
    /// SSH keyboard-interactive or other multi-prompt auth.
    KeyboardInteractive,
    /// Private key passphrase.
    KeyPassphrase,
    Custom(String),
}

impl fmt::Display for CredentialKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UsernamePassword => write!(f, "username/password"),
            Self::BearerToken => write!(f, "bearer token"),
            Self::KeyboardInteractive => write!(f, "keyboard-interactive"),
            Self::KeyPassphrase => write!(f, "key passphrase"),
            Self::Custom(s) => write!(f, "{s}"),
        }
    }
}

/// A single prompt in a multi-prompt authentication challenge.
#[derive(Debug, Clone)]
pub struct AuthPrompt {
    /// The prompt text (e.g., "Password:", "Enter OTP:").
    pub message: String,
    /// Whether the response should be echoed (false = password-style).
    pub echo: bool,
}

/// Information passed to the credential callback when authentication fails.
pub struct CredentialRequest {
    /// The target URI (with any sensitive parts stripped).
    pub url: Url,
    /// The URI scheme ("http", "scp", etc.).
    pub scheme: String,
    /// What kind of credential is needed.
    pub kind: CredentialKind,
    /// Human-readable message (e.g., "Authentication required for example.com").
    pub message: String,
    /// For multi-prompt auth (keyboard-interactive), the individual prompts.
    /// Empty for simple username/password auth.
    pub prompts: Vec<AuthPrompt>,
}

/// Credentials returned by the callback.
#[derive(Debug, Clone, Default)]
pub struct Credentials {
    pub username: Option<String>,
    pub secret: Option<String>,
    /// Responses for multi-prompt auth (keyboard-interactive).
    /// Each entry corresponds to a prompt in `CredentialRequest::prompts`.
    pub responses: Vec<String>,
}

/// Callback type for credential requests.
pub type CredentialCallback = Arc<dyn Fn(&CredentialRequest) -> Option<Credentials> + Send + Sync>;

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
    pub credential_callback: Option<CredentialCallback>,
    /// Content length hint for the body being uploaded.
    /// Enables streaming uploads for handlers that need size upfront (e.g., SCP).
    pub content_length_hint: Option<u64>,
}

impl TransferContext {
    pub fn new(connector: Arc<dyn Connector>) -> Self {
        Self {
            connector,
            progress_sink: None,
            timeout: None,
            options: None,
            credential_callback: None,
            content_length_hint: None,
        }
    }

    /// Downcast scheme-specific options to a concrete type.
    pub fn options<T: 'static>(&self) -> Option<&T> {
        self.options.as_ref()?.downcast_ref::<T>()
    }

    /// Request credentials from the callback (if set).
    /// Returns `None` if no callback is set or the callback declines.
    pub fn request_credentials(&self, request: &CredentialRequest) -> Option<Credentials> {
        self.credential_callback.as_ref()?(request)
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
