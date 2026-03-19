use futures::io::AsyncReadExt;

#[tokio::test]
async fn response_debug_format() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("debug.txt");
    std::fs::write(&path, "debug test").unwrap();

    let uri = format!("file://{}", path.display());
    let response = ayurl::get(&uri).await.unwrap();
    let debug = format!("{response:?}");
    assert!(debug.contains("Response"));
    assert!(debug.contains("content_length"));
}

#[tokio::test]
async fn text_with_invalid_utf8_returns_error() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("binary.bin");
    std::fs::write(&path, b"\xff\xfe\xfd").unwrap();

    let uri = format!("file://{}", path.display());
    let result = ayurl::get(&uri).await.unwrap().text().await;
    assert!(result.is_err());
}

#[tokio::test]
async fn lenient_reader_on_valid_data() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("lenient.txt");
    std::fs::write(&path, "lenient ok").unwrap();

    let uri = format!("file://{}", path.display());
    let response = ayurl::get(&uri).await.unwrap();
    let mut reader = response.lenient_reader();

    let mut buf = Vec::new();
    reader.read_to_end(&mut buf).await.unwrap();
    assert_eq!(buf, b"lenient ok");
}

#[tokio::test]
async fn lenient_reader_returns_eof_on_error() {
    // Create a lenient reader from a reader that immediately errors
    use std::io;
    use std::pin::Pin;
    use std::task::{Context, Poll};

    struct ErrorReader;
    impl futures::io::AsyncRead for ErrorReader {
        fn poll_read(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            _buf: &mut [u8],
        ) -> Poll<io::Result<usize>> {
            Poll::Ready(Err(io::Error::new(io::ErrorKind::BrokenPipe, "test error")))
        }
    }

    let response = ayurl::Response::new(Box::new(ErrorReader), None);
    let mut reader = response.lenient_reader();

    // Should return 0 (EOF) instead of error
    let mut buf = [0u8; 10];
    let n = reader.read(&mut buf).await.unwrap();
    assert_eq!(n, 0);

    // Subsequent reads should also return EOF
    let n = reader.read(&mut buf).await.unwrap();
    assert_eq!(n, 0);
}

#[tokio::test]
async fn reader_returns_raw_asyncread() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("reader.txt");
    std::fs::write(&path, "raw reader").unwrap();

    let uri = format!("file://{}", path.display());
    let response = ayurl::get(&uri).await.unwrap();
    let mut reader = response.reader();

    let mut buf = Vec::new();
    reader.read_to_end(&mut buf).await.unwrap();
    assert_eq!(buf, b"raw reader");
}

#[tokio::test]
async fn response_as_asyncread_directly() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("direct.txt");
    std::fs::write(&path, "direct read").unwrap();

    let uri = format!("file://{}", path.display());
    let mut response = ayurl::get(&uri).await.unwrap();

    let mut buf = Vec::new();
    response.read_to_end(&mut buf).await.unwrap();
    assert_eq!(buf, b"direct read");
}
