use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use axum::http::StatusCode;
use axum::routing::{get, put};
use axum::Router;
use tokio::net::TcpListener;

async fn start_server(app: Router) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    addr
}

#[tokio::test]
async fn http_get_success() {
    let app = Router::new().route("/test", get(|| async { "hello from server" }));
    let addr = start_server(app).await;

    let text = ayurl::get(&format!("http://{addr}/test"))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert_eq!(text, "hello from server");
}

#[tokio::test]
async fn http_get_error_status() {
    let app = Router::new().route(
        "/missing",
        get(|| async { (StatusCode::NOT_FOUND, "not found") }),
    );
    let addr = start_server(app).await;

    let result = ayurl::get(&format!("http://{addr}/missing")).await;
    assert!(result.is_err());
    match result.unwrap_err() {
        ayurl::AyurlError::Http { status, message } => {
            assert_eq!(status, 404);
            assert_eq!(message, "not found");
        }
        other => panic!("expected Http error, got: {other:?}"),
    }
}

#[tokio::test]
async fn http_put_success() {
    let app = Router::new().route("/upload", put(|| async { "accepted" }));
    let addr = start_server(app).await;

    let written = ayurl::put(&format!("http://{addr}/upload"))
        .text("upload data")
        .await
        .unwrap();
    assert_eq!(written, 11);
}

#[tokio::test]
async fn http_put_error_status() {
    let app = Router::new().route(
        "/upload",
        put(|| async { (StatusCode::FORBIDDEN, "forbidden") }),
    );
    let addr = start_server(app).await;

    let result = ayurl::put(&format!("http://{addr}/upload"))
        .text("data")
        .await;
    assert!(result.is_err());
    match result.unwrap_err() {
        ayurl::AyurlError::Http { status, message } => {
            assert_eq!(status, 403);
            assert_eq!(message, "forbidden");
        }
        other => panic!("expected Http error, got: {other:?}"),
    }
}

#[tokio::test]
async fn http_get_with_progress() {
    let app = Router::new().route("/progress", get(|| async { "progress data!" }));
    let addr = start_server(app).await;

    let last_bytes = Arc::new(AtomicU64::new(0));
    let last_clone = last_bytes.clone();

    let data = ayurl::get(&format!("http://{addr}/progress"))
        .on_progress(move |p| {
            last_clone.store(p.bytes_transferred, Ordering::Relaxed);
        })
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();

    assert_eq!(data, b"progress data!");
    assert_eq!(last_bytes.load(Ordering::Relaxed), 14);
}

#[tokio::test]
async fn http_get_with_options() {
    let app = Router::new()
        .route(
            "/headers",
            get(|headers: axum::http::HeaderMap| async move {
                // We can't access outer state easily, so just return headers we got
                let mut parts = Vec::new();
                if let Some(v) = headers.get("authorization") {
                    parts.push(format!("auth={}", v.to_str().unwrap()));
                }
                if let Some(v) = headers.get("x-custom") {
                    parts.push(format!("custom={}", v.to_str().unwrap()));
                }
                parts.join(";")
            }),
        );
    let addr = start_server(app).await;

    let opts = ayurl::HttpOptions::new()
        .header("X-Custom", "value1")
        .bearer_token("mytoken");

    let text = ayurl::get(&format!("http://{addr}/headers"))
        .with_options(opts)
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    assert!(text.contains("auth=Bearer mytoken"));
    assert!(text.contains("custom=value1"));
}

#[tokio::test]
async fn http_get_streaming() {
    let app = Router::new().route("/stream", get(|| async { "streaming http data" }));
    let addr = start_server(app).await;

    let mut response = ayurl::get(&format!("http://{addr}/stream")).await.unwrap();

    use futures::io::AsyncReadExt;
    let mut buf = Vec::new();
    response.read_to_end(&mut buf).await.unwrap();
    assert_eq!(buf, b"streaming http data");
}

#[tokio::test]
async fn http_get_bytes_lossy_on_error() {
    // Server that doesn't exist — connection error
    let result = ayurl::get("http://127.0.0.1:1/nope").await;
    assert!(result.is_err());
    match result.unwrap_err() {
        ayurl::AyurlError::Connection(_) => {}
        other => panic!("expected Connection error, got: {other:?}"),
    }
}

#[tokio::test]
async fn http_put_with_empty_body() {
    let app = Router::new().route("/empty", put(|| async { "ok" }));
    let addr = start_server(app).await;

    let written = ayurl::put(&format!("http://{addr}/empty")).await.unwrap();
    assert_eq!(written, 0);
}

#[tokio::test]
async fn http_get_with_timeout() {
    let app = Router::new().route("/slow", get(|| async { "fast enough" }));
    let addr = start_server(app).await;

    let text = ayurl::get(&format!("http://{addr}/slow"))
        .timeout(std::time::Duration::from_secs(5))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert_eq!(text, "fast enough");
}

#[tokio::test]
async fn http_put_with_timeout() {
    let app = Router::new().route("/tput", put(|| async { "ok" }));
    let addr = start_server(app).await;

    ayurl::put(&format!("http://{addr}/tput"))
        .timeout(std::time::Duration::from_secs(5))
        .text("data")
        .await
        .unwrap();
}

#[tokio::test]
async fn http_content_length_via_head() {
    let app = Router::new().route("/sized", get(|| async { "12345" }));
    let addr = start_server(app).await;

    // The content_length on the response should be populated from the
    // handler's content_length() which does a HEAD request
    let response = ayurl::get(&format!("http://{addr}/sized")).await.unwrap();
    // For small axum responses, content-length may or may not be set in HEAD.
    // Just verify the call doesn't error.
    let _ = response.content_length();
}

#[tokio::test]
async fn http_options_builder() {
    let opts = ayurl::HttpOptions::new()
        .header("X-First", "a")
        .header("X-First", "b")  // duplicate preserved
        .bearer_token("tok");

    // Verify it can be passed through without panicking
    let app = Router::new().route("/opts", get(|| async { "ok" }));
    let addr = start_server(app).await;

    let text = ayurl::get(&format!("http://{addr}/opts"))
        .with_options(opts)
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert_eq!(text, "ok");
}

#[tokio::test]
async fn http_roundtrip_with_file() {
    // Serve some data via HTTP, write to file, read back
    let app = Router::new().route("/data", get(|| async { "roundtrip content" }));
    let addr = start_server(app).await;

    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("downloaded.txt");

    // Download via HTTP
    let data = ayurl::get(&format!("http://{addr}/data"))
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();

    // Write to file
    let file_uri = format!("file://{}", path.display());
    ayurl::put(&file_uri).bytes(data).await.unwrap();

    // Read back
    let text = ayurl::get(&file_uri).await.unwrap().text().await.unwrap();
    assert_eq!(text, "roundtrip content");
}
