use async_trait::async_trait;
use futures::io::AsyncRead;
use futures::stream::TryStreamExt;
use url::Url;

use crate::error::{AyurlError, Result};
use crate::scheme::{SchemeCapabilities, SchemeHandler, TransferContext};

/// Scheme-specific options for HTTP/HTTPS requests.
///
/// Uses a `Vec` internally to preserve header order and allow duplicates.
#[derive(Debug, Clone, Default)]
pub struct HttpOptions {
    headers: Vec<(String, String)>,
}

impl HttpOptions {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a header. Duplicates and ordering are preserved.
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    /// Convenience: set a Bearer token.
    pub fn bearer_token(self, token: impl Into<String>) -> Self {
        self.header("Authorization", format!("Bearer {}", token.into()))
    }
}

/// Handler for `http://` and `https://` URIs using reqwest.
pub struct HttpHandler {
    client: reqwest::Client,
}

impl HttpHandler {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }

    pub fn with_client(client: reqwest::Client) -> Self {
        Self { client }
    }

    fn apply_options(
        builder: reqwest::RequestBuilder,
        ctx: &TransferContext,
    ) -> reqwest::RequestBuilder {
        let mut builder = builder;
        if let Some(opts) = ctx.options::<HttpOptions>() {
            for (name, value) in &opts.headers {
                builder = builder.header(name.as_str(), value.as_str());
            }
        }
        if let Some(timeout) = ctx.timeout {
            builder = builder.timeout(timeout);
        }
        builder
    }
}

impl Default for HttpHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SchemeHandler for HttpHandler {
    async fn get(
        &self,
        uri: &Url,
        ctx: &mut TransferContext,
    ) -> Result<Box<dyn AsyncRead + Send + Unpin>> {
        tracing::debug!(%uri, "http handler: GET");

        let builder = self.client.get(uri.as_str());
        let builder = Self::apply_options(builder, ctx);

        let response = builder.send().await.map_err(|e| {
            AyurlError::Connection(format!("HTTP request failed: {e}"))
        })?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(AyurlError::Http {
                status: status.as_u16(),
                message: body,
            });
        }

        // Convert the reqwest byte stream to an AsyncRead
        let stream = response
            .bytes_stream()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e));
        let reader = tokio_util::io::StreamReader::new(stream);
        let compat = tokio_util::compat::TokioAsyncReadCompatExt::compat(reader);
        Ok(Box::new(compat))
    }

    async fn put(
        &self,
        uri: &Url,
        mut body: Box<dyn AsyncRead + Send + Unpin>,
        ctx: &mut TransferContext,
    ) -> Result<u64> {
        tracing::debug!(%uri, "http handler: PUT");

        // Read the body into memory for now.
        // TODO: streaming upload via reqwest Body::wrap_stream
        let mut buf = Vec::new();
        futures::io::AsyncReadExt::read_to_end(&mut body, &mut buf).await?;
        let len = buf.len() as u64;

        let builder = self.client.put(uri.as_str()).body(buf);
        let builder = Self::apply_options(builder, ctx);

        let response = builder.send().await.map_err(|e| {
            AyurlError::Connection(format!("HTTP PUT failed: {e}"))
        })?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(AyurlError::Http {
                status: status.as_u16(),
                message: body,
            });
        }

        Ok(len)
    }

    async fn content_length(&self, uri: &Url) -> Result<Option<u64>> {
        let response = self.client.head(uri.as_str()).send().await.map_err(|e| {
            AyurlError::Connection(format!("HTTP HEAD failed: {e}"))
        })?;

        Ok(response.content_length())
    }

    fn capabilities(&self) -> SchemeCapabilities {
        SchemeCapabilities {
            supports_streaming: true,
            supports_seek: false,
            supports_content_length: true,
        }
    }
}
