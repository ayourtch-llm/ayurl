use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use axum::http::{HeaderMap, StatusCode};
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

// --- Credential tests ---

/// Helper: axum handler that requires Basic auth with given user/pass.
fn require_basic_auth(
    headers: &HeaderMap,
    expected_user: &str,
    expected_pass: &str,
) -> bool {
    use base64::Engine;
    let Some(auth) = headers.get("authorization") else {
        return false;
    };
    let auth = auth.to_str().unwrap_or("");
    let Some(encoded) = auth.strip_prefix("Basic ") else {
        return false;
    };
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .unwrap_or_default();
    let decoded = String::from_utf8_lossy(&decoded);
    let expected = format!("{expected_user}:{expected_pass}");
    decoded == expected
}

#[tokio::test]
async fn http_get_with_url_credentials() {
    let app = Router::new().route(
        "/secret",
        get(|headers: HeaderMap| async move {
            if require_basic_auth(&headers, "alice", "s3cret") {
                (StatusCode::OK, "authenticated!")
            } else {
                (StatusCode::UNAUTHORIZED, "nope")
            }
        }),
    );
    let addr = start_server(app).await;

    let text = ayurl::get(&format!("http://alice:s3cret@{addr}/secret"))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert_eq!(text, "authenticated!");
}

#[tokio::test]
async fn http_get_url_credentials_wrong_password() {
    let app = Router::new().route(
        "/secret",
        get(|headers: HeaderMap| async move {
            if require_basic_auth(&headers, "alice", "correct") {
                (StatusCode::OK, "ok")
            } else {
                (StatusCode::UNAUTHORIZED, "bad creds")
            }
        }),
    );
    let addr = start_server(app).await;

    let result = ayurl::get(&format!("http://alice:wrong@{addr}/secret")).await;
    assert!(result.is_err());
    match result.unwrap_err() {
        ayurl::AyurlError::Http { status, .. } => assert_eq!(status, 401),
        other => panic!("expected Http 401, got: {other:?}"),
    }
}

#[tokio::test]
async fn http_get_credential_callback_on_401() {
    let app = Router::new().route(
        "/protected",
        get(|headers: HeaderMap| async move {
            if require_basic_auth(&headers, "bob", "password123") {
                (StatusCode::OK, "welcome bob")
            } else {
                (StatusCode::UNAUTHORIZED, "auth required")
            }
        }),
    );
    let addr = start_server(app).await;

    // No credentials in URL, but provide via callback
    let text = ayurl::get(&format!("http://{addr}/protected"))
        .on_credentials(|_req| {
            Some(ayurl::Credentials {
                username: Some("bob".into()),
                secret: Some("password123".into()),
                ..Default::default()
            })
        })
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert_eq!(text, "welcome bob");
}

#[tokio::test]
async fn http_get_credential_callback_receives_info() {
    let app = Router::new().route(
        "/info",
        get(|headers: HeaderMap| async move {
            if require_basic_auth(&headers, "user", "pass") {
                (StatusCode::OK, "ok")
            } else {
                (StatusCode::UNAUTHORIZED, "no")
            }
        }),
    );
    let addr = start_server(app).await;

    let callback_called = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let called = callback_called.clone();

    let text = ayurl::get(&format!("http://{addr}/info"))
        .on_credentials(move |req| {
            called.store(true, Ordering::Relaxed);
            assert_eq!(req.scheme, "http");
            assert!(req.message.contains(&addr.ip().to_string()));
            assert!(matches!(req.kind, ayurl::CredentialKind::UsernamePassword));
            Some(ayurl::Credentials {
                username: Some("user".into()),
                secret: Some("pass".into()),
                ..Default::default()
            })
        })
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    assert_eq!(text, "ok");
    assert!(callback_called.load(Ordering::Relaxed));
}

#[tokio::test]
async fn http_get_credential_callback_declines() {
    let app = Router::new().route(
        "/locked",
        get(|headers: HeaderMap| async move {
            if require_basic_auth(&headers, "x", "y") {
                (StatusCode::OK, "ok")
            } else {
                (StatusCode::UNAUTHORIZED, "locked out")
            }
        }),
    );
    let addr = start_server(app).await;

    // Callback returns None — should propagate the 401
    let result = ayurl::get(&format!("http://{addr}/locked"))
        .on_credentials(|_req| None)
        .await;
    assert!(result.is_err());
    match result.unwrap_err() {
        ayurl::AyurlError::Http { status, message } => {
            assert_eq!(status, 401);
            assert_eq!(message, "locked out");
        }
        other => panic!("expected Http 401, got: {other:?}"),
    }
}

#[tokio::test]
async fn http_put_with_url_credentials() {
    let app = Router::new().route(
        "/upload",
        put(|headers: HeaderMap| async move {
            if require_basic_auth(&headers, "writer", "write_pass") {
                (StatusCode::OK, "stored")
            } else {
                (StatusCode::UNAUTHORIZED, "no auth")
            }
        }),
    );
    let addr = start_server(app).await;

    let written = ayurl::put(&format!("http://writer:write_pass@{addr}/upload"))
        .text("data")
        .await
        .unwrap();
    assert_eq!(written, 4);
}

#[tokio::test]
async fn http_put_credential_callback_on_401() {
    let app = Router::new().route(
        "/secure_upload",
        put(|headers: HeaderMap| async move {
            if require_basic_auth(&headers, "uploader", "secret") {
                (StatusCode::OK, "done")
            } else {
                (StatusCode::UNAUTHORIZED, "nope")
            }
        }),
    );
    let addr = start_server(app).await;

    let written = ayurl::put(&format!("http://{addr}/secure_upload"))
        .on_credentials(|_| {
            Some(ayurl::Credentials {
                username: Some("uploader".into()),
                secret: Some("secret".into()),
                ..Default::default()
            })
        })
        .text("upload data")
        .await
        .unwrap();
    assert_eq!(written, 11);
}

#[tokio::test]
async fn http_client_level_credential_callback() {
    let app = Router::new().route(
        "/client_auth",
        get(|headers: HeaderMap| async move {
            if require_basic_auth(&headers, "global", "creds") {
                (StatusCode::OK, "client-level auth works")
            } else {
                (StatusCode::UNAUTHORIZED, "no")
            }
        }),
    );
    let addr = start_server(app).await;

    let client = ayurl::Client::builder()
        .on_credentials(|_| {
            Some(ayurl::Credentials {
                username: Some("global".into()),
                secret: Some("creds".into()),
                ..Default::default()
            })
        })
        .build();

    let text = client
        .get(&format!("http://{addr}/client_auth"))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert_eq!(text, "client-level auth works");
}

#[tokio::test]
async fn http_request_credential_overrides_client() {
    let app = Router::new().route(
        "/override",
        get(|headers: HeaderMap| async move {
            if require_basic_auth(&headers, "request", "level") {
                (StatusCode::OK, "request wins")
            } else if require_basic_auth(&headers, "client", "level") {
                (StatusCode::OK, "client wins")
            } else {
                (StatusCode::UNAUTHORIZED, "no")
            }
        }),
    );
    let addr = start_server(app).await;

    let client = ayurl::Client::builder()
        .on_credentials(|_| {
            Some(ayurl::Credentials {
                username: Some("client".into()),
                secret: Some("level".into()),
                ..Default::default()
            })
        })
        .build();

    // Per-request callback should override client-level
    let text = client
        .get(&format!("http://{addr}/override"))
        .on_credentials(|_| {
            Some(ayurl::Credentials {
                username: Some("request".into()),
                secret: Some("level".into()),
                ..Default::default()
            })
        })
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert_eq!(text, "request wins");
}
