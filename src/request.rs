use std::any::Any;
use std::future::{Future, IntoFuture};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use futures::io::AsyncRead;

use crate::client::Client;
use crate::error::Result;
use crate::progress::{Progress, ProgressSink};
use crate::response::Response;
use crate::scheme::{CredentialCallback, CredentialRequest, Credentials, TransferContext};

/// Builder for a GET request. Implements `IntoFuture` so it can be
/// `.await`ed directly or configured with chained methods first.
pub struct GetRequest {
    uri: String,
    client: Client,
    timeout: Option<Duration>,
    progress: Option<ProgressSink>,
    options: Option<Box<dyn Any + Send + Sync>>,
    credential_callback: Option<CredentialCallback>,
}

impl GetRequest {
    pub(crate) fn new(uri: String, client: Client) -> Self {
        Self {
            uri,
            client,
            timeout: None,
            progress: None,
            options: None,
            credential_callback: None,
        }
    }

    /// Set a timeout for this specific request.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Set a progress callback.
    pub fn on_progress(mut self, cb: impl Fn(&Progress) + Send + Sync + 'static) -> Self {
        self.progress = Some(ProgressSink::Callback(Arc::new(cb)));
        self
    }

    /// Get a progress watch channel. Returns the modified request and a receiver.
    pub fn progress_channel(mut self) -> (Self, tokio::sync::watch::Receiver<Progress>) {
        let (tx, rx) = tokio::sync::watch::channel(Progress {
            bytes_transferred: 0,
            total_bytes: None,
            elapsed: Duration::ZERO,
        });
        self.progress = Some(ProgressSink::Channel(tx));
        (self, rx)
    }

    /// Set scheme-specific options (e.g., `HttpOptions`).
    pub fn with_options<T: Any + Send + Sync>(mut self, options: T) -> Self {
        self.options = Some(Box::new(options));
        self
    }

    /// Set a per-request credential callback (overrides the client-level one).
    pub fn on_credentials(
        mut self,
        cb: impl Fn(&CredentialRequest) -> Option<Credentials> + Send + Sync + 'static,
    ) -> Self {
        self.credential_callback = Some(Arc::new(cb));
        self
    }

    /// Execute the GET request, returning a streaming `Response`.
    async fn execute(self) -> Result<Response> {
        let url = Client::parse_uri(&self.uri)?;
        let scheme = url.scheme().to_string();
        let handler = self.client.handler_for(&scheme)?;

        let preflight_content_length = handler.content_length(&url).await.unwrap_or(None);

        let mut ctx = TransferContext::new(self.client.connector());
        ctx.timeout = self.timeout.or(self.client.default_timeout());
        ctx.options = self.options;
        ctx.credential_callback = self
            .credential_callback
            .or_else(|| self.client.credential_callback());

        let reader = handler.get(&url, &mut ctx).await?;

        // Prefer content_length discovered during get() (e.g., SCP protocol),
        // fall back to the preflight HEAD-style check.
        let content_length = ctx.response_content_length.or(preflight_content_length);

        // Wrap with progress tracking if requested
        let reader: Box<dyn AsyncRead + Send + Unpin> = match self.progress {
            Some(sink) => {
                use crate::progress::ProgressReader;
                Box::new(ProgressReader::new(reader, content_length, sink))
            }
            None => reader,
        };

        Ok(Response::new(reader, content_length))
    }
}

impl IntoFuture for GetRequest {
    type Output = Result<Response>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(self.execute())
    }
}

/// Builder for a PUT request. Implements `IntoFuture` so it can be
/// `.await`ed directly or configured with chained methods first.
pub struct PutRequest {
    uri: String,
    client: Client,
    timeout: Option<Duration>,
    progress: Option<ProgressSink>,
    options: Option<Box<dyn Any + Send + Sync>>,
    credential_callback: Option<CredentialCallback>,
    body: PutBody,
    content_length: Option<u64>,
}

enum PutBody {
    /// No body set yet — will error on execute.
    Empty,
    /// In-memory bytes.
    Bytes(Vec<u8>),
    /// Streaming reader.
    Stream(Box<dyn AsyncRead + Send + Unpin>),
}

impl PutRequest {
    pub(crate) fn new(uri: String, client: Client) -> Self {
        Self {
            uri,
            client,
            timeout: None,
            progress: None,
            options: None,
            credential_callback: None,
            body: PutBody::Empty,
            content_length: None,
        }
    }

    /// Set a timeout for this specific request.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Set a progress callback.
    pub fn on_progress(mut self, cb: impl Fn(&Progress) + Send + Sync + 'static) -> Self {
        self.progress = Some(ProgressSink::Callback(Arc::new(cb)));
        self
    }

    /// Set scheme-specific options.
    pub fn with_options<T: Any + Send + Sync>(mut self, options: T) -> Self {
        self.options = Some(Box::new(options));
        self
    }

    /// Set a per-request credential callback (overrides the client-level one).
    pub fn on_credentials(
        mut self,
        cb: impl Fn(&CredentialRequest) -> Option<Credentials> + Send + Sync + 'static,
    ) -> Self {
        self.credential_callback = Some(Arc::new(cb));
        self
    }

    /// Provide a content length hint for the body.
    ///
    /// Some handlers (e.g., SCP) require the file size upfront to enable
    /// true streaming uploads. Without this hint, they fall back to
    /// buffering the entire body in memory.
    pub fn content_length(mut self, len: u64) -> Self {
        self.content_length = Some(len);
        self
    }

    /// Set the body from in-memory bytes.
    pub fn bytes(mut self, data: impl Into<Vec<u8>>) -> Self {
        self.body = PutBody::Bytes(data.into());
        self
    }

    /// Set the body from a string.
    pub fn text(mut self, data: impl Into<String>) -> Self {
        self.body = PutBody::Bytes(data.into().into_bytes());
        self
    }

    /// Set the body from a streaming reader.
    pub fn stream(mut self, reader: impl AsyncRead + Send + Unpin + 'static) -> Self {
        self.body = PutBody::Stream(Box::new(reader));
        self
    }

    /// Execute the PUT request, returning the number of bytes written.
    async fn execute(self) -> Result<u64> {
        let url = Client::parse_uri(&self.uri)?;
        let scheme = url.scheme().to_string();
        let handler = self.client.handler_for(&scheme)?;

        let mut ctx = TransferContext::new(self.client.connector());
        ctx.timeout = self.timeout.or(self.client.default_timeout());
        ctx.options = self.options;
        ctx.credential_callback = self
            .credential_callback
            .or_else(|| self.client.credential_callback());
        ctx.content_length_hint = self.content_length;

        let body: Box<dyn AsyncRead + Send + Unpin> = match self.body {
            PutBody::Empty => Box::new(futures::io::empty()),
            PutBody::Bytes(data) => Box::new(futures::io::Cursor::new(data)),
            PutBody::Stream(reader) => reader,
        };

        // Wrap with progress tracking if requested
        let body: Box<dyn AsyncRead + Send + Unpin> = match self.progress {
            Some(sink) => {
                use crate::progress::ProgressReader;
                Box::new(ProgressReader::new(body, None, sink))
            }
            None => body,
        };

        handler.put(&url, body, &mut ctx).await
    }
}

impl IntoFuture for PutRequest {
    type Output = Result<u64>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(self.execute())
    }
}
