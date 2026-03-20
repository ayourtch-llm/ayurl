use std::time::Duration;

#[tokio::test]
async fn default_client_works() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("default.txt");
    std::fs::write(&path, "default client").unwrap();

    let uri = format!("file://{}", path.display());
    let text = ayurl::get(&uri).await.unwrap().text().await.unwrap();
    assert_eq!(text, "default client");
}

#[tokio::test]
async fn client_builder_with_timeout() {
    let client = ayurl::Client::builder()
        .timeout(Duration::from_secs(60))
        .build();

    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("timeout.txt");
    std::fs::write(&path, "with timeout").unwrap();

    let uri = format!("file://{}", path.display());
    let text = client.get(&uri).await.unwrap().text().await.unwrap();
    assert_eq!(text, "with timeout");
}

#[tokio::test]
async fn client_builder_custom_scheme_replaces_default() {
    use async_trait::async_trait;
    use ayurl::ParsedUri;
    use futures::io::AsyncRead;

    struct CustomFileHandler;

    #[async_trait]
    impl ayurl::SchemeHandler for CustomFileHandler {
        async fn get(
            &self,
            _uri: &ParsedUri,
            _ctx: &mut ayurl::TransferContext,
        ) -> ayurl::Result<Box<dyn AsyncRead + Send + Unpin>> {
            // Always return "custom" regardless of the URI
            Ok(Box::new(futures::io::Cursor::new(b"custom".to_vec())))
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
        .register_scheme("file", CustomFileHandler)
        .build();

    let text = client
        .get("file:///anything")
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert_eq!(text, "custom");
}

#[tokio::test]
async fn invalid_uri_returns_error() {
    let result = ayurl::get("://bad").await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    match err {
        ayurl::AyurlError::InvalidUri(_) => {
            // Parser rejects "://bad" — invalid scheme
        }
        other => panic!("expected InvalidUri, got: {other:?}"),
    }
}

#[tokio::test]
async fn client_put_works() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("client_put.txt");

    let client = ayurl::Client::builder().build();
    let uri = format!("file://{}", path.display());
    client.put(&uri).text("via client put").await.unwrap();

    let contents = std::fs::read_to_string(&path).unwrap();
    assert_eq!(contents, "via client put");
}
