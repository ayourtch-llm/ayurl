use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use url::Url;

use crate::error::{AyurlError, Result};
use crate::request::{GetRequest, PutRequest};
use crate::scheme::{Connector, DirectConnector, SchemeHandler};

/// Global default client, lazily initialized.
static DEFAULT_CLIENT: OnceLock<Client> = OnceLock::new();

/// A client for performing URI-based data transfers.
///
/// Holds shared configuration: scheme handlers, connector, timeouts,
/// and connection-level settings. Can be used directly or as the
/// backing store for the module-level `get()`/`put()` functions.
#[derive(Clone)]
pub struct Client {
    inner: Arc<ClientInner>,
}

struct ClientInner {
    schemes: HashMap<String, Arc<dyn SchemeHandler>>,
    connector: Arc<dyn Connector>,
    default_timeout: Option<Duration>,
}

impl Client {
    /// Create a new `ClientBuilder`.
    pub fn builder() -> ClientBuilder {
        ClientBuilder::new()
    }

    /// Start building a GET request for the given URI.
    pub fn get(&self, uri: &str) -> GetRequest {
        GetRequest::new(uri.to_string(), self.clone())
    }

    /// Start building a PUT request for the given URI.
    pub fn put(&self, uri: &str) -> PutRequest {
        PutRequest::new(uri.to_string(), self.clone())
    }

    /// Register a scheme handler at runtime.
    pub fn register_scheme(
        &self,
        scheme: &str,
        handler: impl SchemeHandler + 'static,
    ) -> Result<()> {
        // We need interior mutability for post-construction registration.
        // Since ClientInner is behind Arc, we use a separate lock for the scheme map.
        // For now, this requires &mut self or we redesign with RwLock.
        // Let's keep it simple: registration is on the builder; post-build
        // registration goes through a RwLock.
        //
        // Actually, let's just make schemes use an RwLock.
        // This is a design simplification for the initial version.
        // We'll revisit if performance is a concern.
        let _ = (scheme, handler);
        unimplemented!(
            "post-construction registration requires RwLock scheme map; \
             use ClientBuilder::register_scheme() for now"
        )
    }

    /// Look up the handler for a given scheme.
    pub(crate) fn handler_for(&self, scheme: &str) -> Result<Arc<dyn SchemeHandler>> {
        self.inner
            .schemes
            .get(scheme)
            .cloned()
            .ok_or_else(|| AyurlError::UnsupportedScheme(scheme.to_string()))
    }

    /// Get the connector.
    pub(crate) fn connector(&self) -> Arc<dyn Connector> {
        self.inner.connector.clone()
    }

    /// Get the default timeout.
    pub(crate) fn default_timeout(&self) -> Option<Duration> {
        self.inner.default_timeout
    }

    /// Parse and validate a URI string.
    pub(crate) fn parse_uri(uri: &str) -> Result<Url> {
        // Handle file:// URIs with relative paths
        Url::parse(uri).map_err(|e| AyurlError::InvalidUri(format!("{uri}: {e}")))
    }
}

impl Default for Client {
    fn default() -> Self {
        ClientBuilder::new().build()
    }
}

/// Builder for constructing a `Client` with custom configuration.
pub struct ClientBuilder {
    schemes: HashMap<String, Arc<dyn SchemeHandler>>,
    connector: Option<Arc<dyn Connector>>,
    default_timeout: Option<Duration>,
}

impl ClientBuilder {
    pub fn new() -> Self {
        Self {
            schemes: HashMap::new(),
            connector: None,
            default_timeout: None,
        }
    }

    /// Register a scheme handler (type-safe: must impl `SchemeHandler`).
    pub fn register_scheme(
        mut self,
        scheme: &str,
        handler: impl SchemeHandler + 'static,
    ) -> Self {
        self.schemes.insert(scheme.to_string(), Arc::new(handler));
        self
    }

    /// Set the transport connector (for tunneling, proxying, etc.).
    pub fn connector(mut self, connector: impl Connector + 'static) -> Self {
        self.connector = Some(Arc::new(connector));
        self
    }

    /// Set the default timeout for all transfers.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.default_timeout = Some(timeout);
        self
    }

    /// Build the `Client`, registering default handlers for enabled features.
    pub fn build(mut self) -> Client {
        // Register default handlers for enabled features (if not already set)
        #[cfg(feature = "file")]
        {
            self.schemes
                .entry("file".to_string())
                .or_insert_with(|| Arc::new(crate::handlers::file::FileHandler));
        }

        #[cfg(feature = "http")]
        {
            let http_handler = Arc::new(crate::handlers::http::HttpHandler::new());
            self.schemes
                .entry("http".to_string())
                .or_insert_with(|| http_handler.clone());
            self.schemes
                .entry("https".to_string())
                .or_insert_with(|| http_handler);
        }

        Client {
            inner: Arc::new(ClientInner {
                schemes: self.schemes,
                connector: self
                    .connector
                    .unwrap_or_else(|| Arc::new(DirectConnector)),
                default_timeout: self.default_timeout,
            }),
        }
    }
}

impl Default for ClientBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Get or lazily initialize the global default client.
pub(crate) fn default_client() -> &'static Client {
    DEFAULT_CLIENT.get_or_init(Client::default)
}

/// Configure the global default client before first use.
///
/// Returns `Err(AyurlError::AlreadyConfigured)` if the default client
/// has already been initialized (either explicitly or by a prior `get()`/`put()` call).
pub fn configure_default<F>(f: F) -> Result<()>
where
    F: FnOnce(ClientBuilder) -> ClientBuilder,
{
    let builder = f(ClientBuilder::new());
    let client = builder.build();
    DEFAULT_CLIENT
        .set(client)
        .map_err(|_| AyurlError::AlreadyConfigured)
}
