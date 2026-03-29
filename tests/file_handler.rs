use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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

/// Verify that concurrent readers never see empty or partial content during a PUT.
///
/// Strategy: pre-populate a file with known content, then hammer it with
/// concurrent PUTs (large payload) while a reader loop keeps reading.
/// With atomic writes (temp+rename), every read returns either the old
/// content or the new content — never empty or truncated.
#[tokio::test]
async fn put_is_atomic_concurrent_readers_never_see_partial() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("atomic_test.json");
    let uri = format!("file://{}", path.display());

    // Initial content — something we can validate
    let initial = "A".repeat(64 * 1024); // 64 KiB of 'A's
    std::fs::write(&path, &initial).unwrap();

    // New content — different, same size
    let replacement = "B".repeat(64 * 1024); // 64 KiB of 'B's

    let saw_partial = Arc::new(AtomicBool::new(false));
    let stop = Arc::new(AtomicBool::new(false));

    // Spawn a reader task that continuously reads the file
    let reader_path = path.clone();
    let saw_partial_clone = saw_partial.clone();
    let stop_clone = stop.clone();
    let initial_clone = initial.clone();
    let replacement_clone = replacement.clone();
    let reader = tokio::spawn(async move {
        let mut reads = 0u64;
        while !stop_clone.load(Ordering::Relaxed) {
            match tokio::fs::read(&reader_path).await {
                Ok(data) => {
                    reads += 1;
                    let is_initial = data == initial_clone.as_bytes();
                    let is_replacement = data == replacement_clone.as_bytes();
                    if !is_initial && !is_replacement {
                        saw_partial_clone.store(true, Ordering::Relaxed);
                        // Don't break — keep reading to increase confidence
                    }
                }
                Err(_) => {
                    // File momentarily missing would also be a failure for atomic writes,
                    // but we don't count it here since rename is atomic on Linux.
                }
            }
            tokio::task::yield_now().await;
        }
        reads
    });

    // Perform many sequential PUTs to maximize the race window
    for _ in 0..20 {
        ayurl::put(&uri)
            .text(replacement.clone())
            .await
            .unwrap();
    }

    stop.store(true, Ordering::Relaxed);
    let reads = reader.await.unwrap();

    assert!(
        reads > 0,
        "reader should have completed at least one read"
    );
    assert!(
        !saw_partial.load(Ordering::Relaxed),
        "reader saw partial/empty content during PUT — writes are not atomic! ({} reads performed)",
        reads
    );
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
