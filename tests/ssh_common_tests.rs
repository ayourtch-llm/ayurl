use ayurl::handlers::ssh_common::{parse_ssh_url, SshOptions};
use ayurl::ParsedUri;

// --- SshOptions ---

#[test]
fn ssh_options_default() {
    let opts = SshOptions::new();
    assert!(opts.private_key.is_none());
    assert!(opts.private_key_path.is_none());
    assert!(opts.file_mode.is_none());
}

#[test]
fn ssh_options_with_private_key() {
    let opts = SshOptions::new().with_private_key(b"key-data".to_vec());
    assert_eq!(opts.private_key.as_deref(), Some(b"key-data".as_slice()));
}

#[test]
fn ssh_options_with_private_key_path() {
    let opts = SshOptions::new().with_private_key_path("/home/user/.ssh/id_ed25519");
    assert_eq!(
        opts.private_key_path.as_deref(),
        Some(std::path::Path::new("/home/user/.ssh/id_ed25519"))
    );
}

#[test]
fn ssh_options_with_file_mode() {
    let opts = SshOptions::new().with_file_mode(0o755);
    assert_eq!(opts.file_mode, Some(0o755));
}

#[test]
fn ssh_options_chained() {
    let opts = SshOptions::new()
        .with_private_key(b"key".to_vec())
        .with_file_mode(0o600);
    assert!(opts.private_key.is_some());
    assert_eq!(opts.file_mode, Some(0o600));
}

#[tokio::test]
async fn ssh_options_load_private_key_from_memory() {
    let opts = SshOptions::new().with_private_key(b"in-memory-key".to_vec());
    let key = opts.load_private_key().await.unwrap();
    assert_eq!(key.as_deref(), Some(b"in-memory-key".as_slice()));
}

#[tokio::test]
async fn ssh_options_load_private_key_from_file() {
    let dir = tempfile::TempDir::new().unwrap();
    let key_path = dir.path().join("test_key");
    std::fs::write(&key_path, b"file-key-data").unwrap();

    let opts = SshOptions::new().with_private_key_path(&key_path);
    let key = opts.load_private_key().await.unwrap();
    assert_eq!(key.as_deref(), Some(b"file-key-data".as_slice()));
}

#[tokio::test]
async fn ssh_options_load_private_key_none() {
    let opts = SshOptions::new();
    let key = opts.load_private_key().await.unwrap();
    assert!(key.is_none());
}

#[tokio::test]
async fn ssh_options_load_private_key_file_not_found() {
    let opts = SshOptions::new().with_private_key_path("/nonexistent/key");
    let result = opts.load_private_key().await;
    assert!(result.is_err());
}

#[tokio::test]
async fn ssh_options_memory_takes_precedence_over_path() {
    let opts = SshOptions::new()
        .with_private_key(b"memory-key".to_vec())
        .with_private_key_path("/nonexistent/key");
    // Memory key should be returned without trying the file
    let key = opts.load_private_key().await.unwrap();
    assert_eq!(key.as_deref(), Some(b"memory-key".as_slice()));
}

// --- parse_ssh_url ---

#[test]
fn parse_basic_scp_url() {
    let uri = ParsedUri::parse("scp://user@host/path/file.txt").unwrap();
    let target = parse_ssh_url(&uri).unwrap();
    assert_eq!(target.host, "host");
    assert_eq!(target.port, 22);
    assert_eq!(target.username, "user");
    assert_eq!(target.path, "path/file.txt");
    assert!(target.password.is_none());
}

#[test]
fn parse_scp_with_password() {
    let uri = ParsedUri::parse("scp://user:pass@host/file").unwrap();
    let target = parse_ssh_url(&uri).unwrap();
    assert_eq!(target.username, "user");
    assert_eq!(target.password, Some("pass".to_string()));
}

#[test]
fn parse_scp_with_port() {
    let uri = ParsedUri::parse("scp://user@host:2222/file").unwrap();
    let target = parse_ssh_url(&uri).unwrap();
    assert_eq!(target.port, 2222);
}

#[test]
fn parse_scp_absolute_path() {
    // Double slash = absolute path on remote
    let uri = ParsedUri::parse("scp://user@host//tmp/file").unwrap();
    let target = parse_ssh_url(&uri).unwrap();
    assert_eq!(target.path, "/tmp/file");
}

#[test]
fn parse_sftp_url() {
    let uri = ParsedUri::parse("sftp://admin@server.example.com/data/file.csv").unwrap();
    let target = parse_ssh_url(&uri).unwrap();
    assert_eq!(target.host, "server.example.com");
    assert_eq!(target.username, "admin");
    assert_eq!(target.path, "data/file.csv");
}

#[test]
fn parse_scp_no_user_falls_back() {
    let uri = ParsedUri::parse("scp://host/file").unwrap();
    let target = parse_ssh_url(&uri).unwrap();
    // Should use env USER or fallback
    assert!(!target.username.is_empty());
}

#[test]
fn parse_scp_missing_path_errors() {
    let uri = ParsedUri::parse("scp://user@host/").unwrap();
    let result = parse_ssh_url(&uri);
    assert!(result.is_err());
}

#[test]
fn parse_scp_ipv6() {
    let uri = ParsedUri::parse("scp://user@[::1]:22/file").unwrap();
    let target = parse_ssh_url(&uri).unwrap();
    assert_eq!(target.host, "::1");
    assert_eq!(target.port, 22);
}
