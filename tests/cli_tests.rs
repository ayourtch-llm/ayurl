use ayurl::cli::{normalize_uri, run_copy, run_get, Cli, Command};
use clap::Parser;
use tempfile::TempDir;

// --- normalize_uri tests ---

#[test]
fn normalize_uri_with_scheme_unchanged() {
    assert_eq!(normalize_uri("http://example.com/path"), "http://example.com/path");
    assert_eq!(normalize_uri("file:///tmp/foo"), "file:///tmp/foo");
    assert_eq!(normalize_uri("scp://host/path"), "scp://host/path");
}

#[test]
fn normalize_uri_absolute_path() {
    let result = normalize_uri("/tmp/some/file.txt");
    assert_eq!(result, "file:///tmp/some/file.txt");
}

#[test]
fn normalize_uri_relative_path() {
    let result = normalize_uri("relative/file.txt");
    assert!(result.starts_with("file://"));
    assert!(result.ends_with("relative/file.txt"));
    // Should contain the current directory
    let cwd = std::env::current_dir().unwrap();
    assert!(result.contains(&cwd.display().to_string()));
}

// --- clap parsing tests ---

#[test]
fn parse_copy_command() {
    let cli = Cli::parse_from(["ayurl", "copy", "src.txt", "dst.txt"]);
    match cli.command {
        Command::Copy { src, dst, progress } => {
            assert_eq!(src, "src.txt");
            assert_eq!(dst, "dst.txt");
            assert!(!progress);
        }
        _ => panic!("expected Copy command"),
    }
}

#[test]
fn parse_copy_with_progress() {
    let cli = Cli::parse_from(["ayurl", "copy", "-p", "src", "dst"]);
    match cli.command {
        Command::Copy { progress, .. } => assert!(progress),
        _ => panic!("expected Copy"),
    }
}

#[test]
fn parse_cp_alias() {
    let cli = Cli::parse_from(["ayurl", "cp", "a", "b"]);
    assert!(matches!(cli.command, Command::Copy { .. }));
}

#[test]
fn parse_get_command() {
    let cli = Cli::parse_from(["ayurl", "get", "http://example.com"]);
    match cli.command {
        Command::Get { uri, progress } => {
            assert_eq!(uri, "http://example.com");
            assert!(!progress);
        }
        _ => panic!("expected Get"),
    }
}

#[test]
fn parse_cat_alias() {
    let cli = Cli::parse_from(["ayurl", "cat", "file.txt"]);
    assert!(matches!(cli.command, Command::Get { .. }));
}

#[test]
fn parse_put_command() {
    let cli = Cli::parse_from(["ayurl", "put", "file:///tmp/out"]);
    match cli.command {
        Command::Put { uri, progress } => {
            assert_eq!(uri, "file:///tmp/out");
            assert!(!progress);
        }
        _ => panic!("expected Put"),
    }
}

#[test]
fn parse_get_with_progress() {
    let cli = Cli::parse_from(["ayurl", "get", "-p", "http://example.com"]);
    match cli.command {
        Command::Get { progress, .. } => assert!(progress),
        _ => panic!("expected Get"),
    }
}

// --- run_copy tests ---

#[tokio::test]
async fn copy_file_to_file() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("src.txt");
    let dst = dir.path().join("dst.txt");
    std::fs::write(&src, "copy me").unwrap();

    let bytes = run_copy(
        src.to_str().unwrap(),
        dst.to_str().unwrap(),
        false,
    )
    .await
    .unwrap();

    assert_eq!(bytes, 7);
    assert_eq!(std::fs::read_to_string(&dst).unwrap(), "copy me");
}

#[tokio::test]
async fn copy_file_to_file_with_progress() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("src.txt");
    let dst = dir.path().join("dst.txt");
    std::fs::write(&src, "progress copy").unwrap();

    let bytes = run_copy(
        src.to_str().unwrap(),
        dst.to_str().unwrap(),
        true,
    )
    .await
    .unwrap();

    assert_eq!(bytes, 13);
    assert_eq!(std::fs::read_to_string(&dst).unwrap(), "progress copy");
}

#[tokio::test]
async fn copy_with_uri_syntax() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("src.txt");
    let dst = dir.path().join("dst.txt");
    std::fs::write(&src, "uri syntax").unwrap();

    let src_uri = format!("file://{}", src.display());
    let dst_uri = format!("file://{}", dst.display());

    let bytes = run_copy(&src_uri, &dst_uri, false).await.unwrap();
    assert_eq!(bytes, 10);
}

#[tokio::test]
async fn copy_creates_destination_dirs() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("src.txt");
    let dst = dir.path().join("sub").join("dir").join("dst.txt");
    std::fs::write(&src, "nested").unwrap();

    let bytes = run_copy(
        src.to_str().unwrap(),
        dst.to_str().unwrap(),
        false,
    )
    .await
    .unwrap();

    assert_eq!(bytes, 6);
    assert_eq!(std::fs::read_to_string(&dst).unwrap(), "nested");
}

#[tokio::test]
async fn copy_nonexistent_source_errors() {
    let dir = TempDir::new().unwrap();
    let dst = dir.path().join("dst.txt");

    let result = run_copy("/nonexistent/file.txt", dst.to_str().unwrap(), false).await;
    assert!(result.is_err());
}

// --- run_get tests ---

#[tokio::test]
async fn get_file_to_stdout() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("get.txt");
    std::fs::write(&path, "get output").unwrap();

    // run_get writes to stdout; just verify it doesn't error
    run_get(path.to_str().unwrap(), false).await.unwrap();
}

#[tokio::test]
async fn get_with_progress() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("get_p.txt");
    std::fs::write(&path, "get progress").unwrap();

    run_get(path.to_str().unwrap(), true).await.unwrap();
}

#[tokio::test]
async fn get_nonexistent_errors() {
    let result = run_get("/nonexistent/path.txt", false).await;
    assert!(result.is_err());
}

// --- copy with HTTP (integration) ---

#[tokio::test]
async fn copy_http_to_file() {
    use axum::routing::get;
    use axum::Router;
    use tokio::net::TcpListener;

    let app = Router::new().route("/data", get(|| async { "http content" }));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let dir = TempDir::new().unwrap();
    let dst = dir.path().join("from_http.txt");

    let bytes = run_copy(
        &format!("http://{addr}/data"),
        dst.to_str().unwrap(),
        false,
    )
    .await
    .unwrap();

    assert_eq!(bytes, 12);
    assert_eq!(std::fs::read_to_string(&dst).unwrap(), "http content");
}

#[tokio::test]
async fn copy_large_file() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("large.bin");
    let dst = dir.path().join("large_copy.bin");

    // Create a 1MB file
    let data = vec![0xABu8; 1024 * 1024];
    std::fs::write(&src, &data).unwrap();

    let bytes = run_copy(
        src.to_str().unwrap(),
        dst.to_str().unwrap(),
        true,
    )
    .await
    .unwrap();

    assert_eq!(bytes, 1024 * 1024);
    assert_eq!(std::fs::read(&dst).unwrap(), data);
}
