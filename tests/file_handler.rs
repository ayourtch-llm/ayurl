use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use futures::io::AsyncReadExt;
use tempfile::TempDir;

#[tokio::test]
async fn get_file_bytes() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("hello.txt");
    std::fs::write(&path, "hello world").unwrap();

    let uri = format!("file://{}", path.display());
    let data = ayurl::get(&uri).await.unwrap().bytes().await.unwrap();
    assert_eq!(data, b"hello world");
}

#[tokio::test]
async fn get_file_text() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("greeting.txt");
    std::fs::write(&path, "hej verden").unwrap();

    let uri = format!("file://{}", path.display());
    let text = ayurl::get(&uri).await.unwrap().text().await.unwrap();
    assert_eq!(text, "hej verden");
}

#[tokio::test]
async fn put_file_bytes() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("output.bin");

    let uri = format!("file://{}", path.display());
    let written = ayurl::put(&uri).bytes(b"binary data".to_vec()).await.unwrap();
    assert_eq!(written, 11);

    let contents = std::fs::read(&path).unwrap();
    assert_eq!(contents, b"binary data");
}

#[tokio::test]
async fn put_file_text() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("output.txt");

    let uri = format!("file://{}", path.display());
    ayurl::put(&uri).text("hello from ayurl").await.unwrap();

    let contents = std::fs::read_to_string(&path).unwrap();
    assert_eq!(contents, "hello from ayurl");
}

#[tokio::test]
async fn roundtrip_file() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("src.txt");
    let dst = dir.path().join("dst.txt");
    std::fs::write(&src, "roundtrip data").unwrap();

    let src_uri = format!("file://{}", src.display());
    let dst_uri = format!("file://{}", dst.display());

    let data = ayurl::get(&src_uri).await.unwrap().bytes().await.unwrap();
    ayurl::put(&dst_uri).bytes(data).await.unwrap();

    let result = std::fs::read_to_string(&dst).unwrap();
    assert_eq!(result, "roundtrip data");
}

#[tokio::test]
async fn streaming_read() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("stream.txt");
    std::fs::write(&path, "streaming content").unwrap();

    let uri = format!("file://{}", path.display());
    let mut response = ayurl::get(&uri).await.unwrap();

    // Read in chunks using the AsyncRead interface
    let mut buf = [0u8; 4];
    let mut collected = Vec::new();
    loop {
        let n = response.read(&mut buf).await.unwrap();
        if n == 0 {
            break;
        }
        collected.extend_from_slice(&buf[..n]);
    }
    assert_eq!(collected, b"streaming content");
}

#[tokio::test]
async fn progress_callback() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("progress.txt");
    std::fs::write(&path, "some data for progress tracking").unwrap();

    let last_bytes = Arc::new(AtomicU64::new(0));
    let last_bytes_clone = last_bytes.clone();

    let uri = format!("file://{}", path.display());
    let data = ayurl::get(&uri)
        .on_progress(move |p| {
            last_bytes_clone.store(p.bytes_transferred, Ordering::Relaxed);
        })
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();

    let expected_len = "some data for progress tracking".len();
    assert_eq!(data.len(), expected_len);
    assert_eq!(last_bytes.load(Ordering::Relaxed), expected_len as u64);
}

#[tokio::test]
async fn get_nonexistent_file_errors() {
    let result = ayurl::get("file:///nonexistent/path/to/file.txt").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn bytes_lossy_on_success() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("lossy.txt");
    std::fs::write(&path, "lossy ok").unwrap();

    let uri = format!("file://{}", path.display());
    let data = ayurl::get(&uri).await.unwrap().bytes_lossy().await;
    assert_eq!(data, b"lossy ok");
}

#[tokio::test]
async fn text_lossy_on_success() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("lossy.txt");
    std::fs::write(&path, "lossy text ok").unwrap();

    let uri = format!("file://{}", path.display());
    let text = ayurl::get(&uri).await.unwrap().text_lossy().await;
    assert_eq!(text, "lossy text ok");
}

#[tokio::test]
async fn text_lossy_with_invalid_utf8() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("binary.bin");
    std::fs::write(&path, b"\xff\xfe hello \xff").unwrap();

    let uri = format!("file://{}", path.display());
    let text = ayurl::get(&uri).await.unwrap().text_lossy().await;
    // Should contain replacement characters but not fail
    assert!(text.contains("hello"));
}

#[tokio::test]
async fn content_length_known_for_files() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("sized.txt");
    std::fs::write(&path, "12345").unwrap();

    let uri = format!("file://{}", path.display());
    let response = ayurl::get(&uri).await.unwrap();
    assert_eq!(response.content_length(), Some(5));
}

#[tokio::test]
async fn unsupported_scheme_errors() {
    let result = ayurl::get("ftp://example.com/file").await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(matches!(err, ayurl::AyurlError::UnsupportedScheme(_)));
}

#[tokio::test]
async fn put_creates_parent_dirs() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("sub").join("dir").join("file.txt");

    let uri = format!("file://{}", path.display());
    ayurl::put(&uri).text("nested").await.unwrap();

    let contents = std::fs::read_to_string(&path).unwrap();
    assert_eq!(contents, "nested");
}

#[tokio::test]
async fn explicit_client() {
    let client = ayurl::Client::builder().build();

    let dir = TempDir::new().unwrap();
    let path = dir.path().join("client.txt");
    std::fs::write(&path, "via client").unwrap();

    let uri = format!("file://{}", path.display());
    let text = client.get(&uri).await.unwrap().text().await.unwrap();
    assert_eq!(text, "via client");
}
