use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[tokio::test]
async fn get_request_with_timeout() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("timeout.txt");
    std::fs::write(&path, "timeout test").unwrap();

    let uri = format!("file://{}", path.display());
    let text = ayurl::get(&uri)
        .timeout(Duration::from_secs(10))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert_eq!(text, "timeout test");
}

#[tokio::test]
async fn put_request_with_bytes() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("put_bytes.txt");

    let uri = format!("file://{}", path.display());
    let written = ayurl::put(&uri)
        .bytes(b"put bytes data".to_vec())
        .await
        .unwrap();
    assert_eq!(written, 14);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "put bytes data");
}

#[tokio::test]
async fn put_request_with_text() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("put_text.txt");

    let uri = format!("file://{}", path.display());
    ayurl::put(&uri)
        .text("put text data")
        .await
        .unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "put text data");
}

#[tokio::test]
async fn put_request_with_stream() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("put_stream.txt");

    let uri = format!("file://{}", path.display());
    let cursor = futures::io::Cursor::new(b"streamed data".to_vec());
    ayurl::put(&uri)
        .stream(cursor)
        .await
        .unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "streamed data");
}

#[tokio::test]
async fn put_empty_body() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("empty.txt");

    let uri = format!("file://{}", path.display());
    let written = ayurl::put(&uri).await.unwrap();
    assert_eq!(written, 0);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "");
}

#[tokio::test]
async fn put_request_with_timeout() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("put_timeout.txt");

    let uri = format!("file://{}", path.display());
    ayurl::put(&uri)
        .timeout(Duration::from_secs(10))
        .text("with timeout")
        .await
        .unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "with timeout");
}

#[tokio::test]
async fn put_request_with_progress() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("put_progress.txt");
    let data = "progress tracking on put";

    let last_bytes = Arc::new(AtomicU64::new(0));
    let last_bytes_clone = last_bytes.clone();

    let uri = format!("file://{}", path.display());
    ayurl::put(&uri)
        .on_progress(move |p| {
            last_bytes_clone.store(p.bytes_transferred, Ordering::Relaxed);
        })
        .text(data)
        .await
        .unwrap();

    assert_eq!(
        last_bytes.load(Ordering::Relaxed),
        data.len() as u64
    );
}

#[tokio::test]
async fn get_request_with_progress_channel() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("channel.txt");
    std::fs::write(&path, "channel progress data").unwrap();

    let uri = format!("file://{}", path.display());
    let (req, rx) = ayurl::get(&uri).progress_channel();

    let data = req.await.unwrap().bytes().await.unwrap();
    assert_eq!(data, b"channel progress data");

    // The channel should have the final progress
    let progress = rx.borrow().clone();
    assert_eq!(progress.bytes_transferred, 21);
}

#[tokio::test]
async fn put_request_with_options() {
    // with_options shouldn't break file:// handler (options are ignored)
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("options.txt");

    let uri = format!("file://{}", path.display());
    ayurl::put(&uri)
        .with_options("some option".to_string())
        .text("with options")
        .await
        .unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "with options");
}

#[tokio::test]
async fn get_request_with_options() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("get_opts.txt");
    std::fs::write(&path, "options").unwrap();

    let uri = format!("file://{}", path.display());
    let text = ayurl::get(&uri)
        .with_options(42u32)
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert_eq!(text, "options");
}
