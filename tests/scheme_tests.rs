use std::sync::Arc;

use async_trait::async_trait;
use ayurl::{ParsedUri, SchemeHandler};
use futures::io::AsyncRead;

/// A minimal handler that only implements required methods (to test defaults)
struct MinimalHandler;

#[async_trait]
impl ayurl::SchemeHandler for MinimalHandler {
    async fn get(
        &self,
        _uri: &ParsedUri,
        _ctx: &mut ayurl::TransferContext,
    ) -> ayurl::Result<Box<dyn AsyncRead + Send + Unpin>> {
        Ok(Box::new(futures::io::Cursor::new(b"minimal".to_vec())))
    }

    async fn put(
        &self,
        _uri: &ParsedUri,
        _body: Box<dyn AsyncRead + Send + Unpin>,
        _ctx: &mut ayurl::TransferContext,
    ) -> ayurl::Result<u64> {
        Ok(0)
    }
}

#[tokio::test]
async fn default_content_length_returns_none() {
    let handler = MinimalHandler;
    let uri = ParsedUri::parse("test:///path").unwrap();
    let result = handler.content_length(&uri).await.unwrap();
    assert_eq!(result, None);
}

#[tokio::test]
async fn default_capabilities() {
    let handler = MinimalHandler;
    let caps = handler.capabilities();
    assert!(caps.supports_streaming);
    assert!(!caps.supports_seek);
    assert!(!caps.supports_content_length);
}

#[tokio::test]
async fn transfer_context_options_downcast() {
    let connector = Arc::new(ayurl::DirectConnector);
    let mut ctx = ayurl::TransferContext::new(connector);

    // No options set
    assert!(ctx.options::<String>().is_none());

    // Set options
    ctx.options = Some(Box::new("hello".to_string()));
    assert_eq!(ctx.options::<String>().unwrap(), "hello");

    // Wrong type returns None
    assert!(ctx.options::<u32>().is_none());
}

#[tokio::test]
async fn scheme_capabilities_default() {
    let caps = ayurl::SchemeCapabilities::default();
    assert!(caps.supports_streaming);
    assert!(!caps.supports_seek);
    assert!(!caps.supports_content_length);
}

#[tokio::test]
async fn custom_scheme_handler() {
    struct EchoHandler;

    #[async_trait]
    impl ayurl::SchemeHandler for EchoHandler {
        async fn get(
            &self,
            uri: &ParsedUri,
            _ctx: &mut ayurl::TransferContext,
        ) -> ayurl::Result<Box<dyn AsyncRead + Send + Unpin>> {
            // Return the path as the content
            let content = uri.path().as_bytes().to_vec();
            Ok(Box::new(futures::io::Cursor::new(content)))
        }

        async fn put(
            &self,
            _uri: &ParsedUri,
            mut body: Box<dyn AsyncRead + Send + Unpin>,
            _ctx: &mut ayurl::TransferContext,
        ) -> ayurl::Result<u64> {
            use futures::io::AsyncReadExt;
            let mut buf = Vec::new();
            body.read_to_end(&mut buf).await?;
            Ok(buf.len() as u64)
        }

        async fn content_length(&self, uri: &ParsedUri) -> ayurl::Result<Option<u64>> {
            Ok(Some(uri.path().len() as u64))
        }

        fn capabilities(&self) -> ayurl::SchemeCapabilities {
            ayurl::SchemeCapabilities {
                supports_streaming: true,
                supports_seek: false,
                supports_content_length: true,
            }
        }
    }

    let client = ayurl::Client::builder()
        .register_scheme("echo", EchoHandler)
        .build();

    // GET
    let text = client
        .get("echo:///hello/world")
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert_eq!(text, "/hello/world");

    // PUT
    let written = client
        .put("echo:///test")
        .text("some data")
        .await
        .unwrap();
    assert_eq!(written, 9);

    // Content length is reflected in response
    let response = client.get("echo:///abc").await.unwrap();
    assert_eq!(response.content_length(), Some(4));
}

#[tokio::test]
async fn multiple_custom_schemes() {
    struct ConstHandler(&'static str);

    #[async_trait]
    impl ayurl::SchemeHandler for ConstHandler {
        async fn get(
            &self,
            _uri: &ParsedUri,
            _ctx: &mut ayurl::TransferContext,
        ) -> ayurl::Result<Box<dyn AsyncRead + Send + Unpin>> {
            Ok(Box::new(futures::io::Cursor::new(
                self.0.as_bytes().to_vec(),
            )))
        }

        async fn put(
            &self,
            _uri: &ParsedUri,
            _body: Box<dyn AsyncRead + Send + Unpin>,
            _ctx: &mut ayurl::TransferContext,
        ) -> ayurl::Result<u64> {
            Ok(0)
        }
    }

    let client = ayurl::Client::builder()
        .register_scheme("alpha", ConstHandler("from alpha"))
        .register_scheme("beta", ConstHandler("from beta"))
        .build();

    let a = client.get("alpha:///x").await.unwrap().text().await.unwrap();
    let b = client.get("beta:///y").await.unwrap().text().await.unwrap();
    assert_eq!(a, "from alpha");
    assert_eq!(b, "from beta");
}
