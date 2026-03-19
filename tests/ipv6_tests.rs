/// Tests for IPv6 address handling in URIs.

// --- URL parsing (verifying url crate behavior) ---

#[test]
fn url_crate_ipv6_scp() {
    // url crate keeps brackets for non-standard schemes
    let url = url::Url::parse("scp://user@[::1]/path/to/file").unwrap();
    assert_eq!(url.host_str(), Some("[::1]"));
    assert_eq!(url.username(), "user");
    assert_eq!(url.path(), "/path/to/file");
}

#[test]
fn url_crate_ipv6_http() {
    // url crate keeps brackets in host_str() for IPv6
    let url = url::Url::parse("http://[::1]:8080/path").unwrap();
    assert_eq!(url.host_str(), Some("[::1]"));
    assert_eq!(url.port(), Some(8080));
    // But Url::host() returns the parsed Ipv6Addr without brackets
    assert!(matches!(url.host(), Some(url::Host::Ipv6(_))));
}

// --- parse_ssh_url strips brackets ---

#[tokio::test]
async fn ssh_parse_ipv6_loopback() {
    let url = url::Url::parse("scp://testuser@[::1]:2222/remote/path").unwrap();
    let target = ayurl::handlers::ssh_common::parse_ssh_url(&url).unwrap();
    assert_eq!(target.host, "::1");
    assert_eq!(target.port, 2222);
    assert_eq!(target.username, "testuser");
    assert_eq!(target.path, "remote/path");
}

#[tokio::test]
async fn ssh_parse_ipv6_full_address() {
    let url =
        url::Url::parse("scp://user@[2a02:1811:1c88:7500:ceb7:d034:7ecb:4e30]//tmp/hugefile")
            .unwrap();
    let target = ayurl::handlers::ssh_common::parse_ssh_url(&url).unwrap();
    assert_eq!(target.host, "2a02:1811:1c88:7500:ceb7:d034:7ecb:4e30");
    assert_eq!(target.port, 22);
    assert_eq!(target.username, "user");
    assert_eq!(target.path, "/tmp/hugefile");
}

#[tokio::test]
async fn ssh_parse_ipv6_with_password() {
    let url = url::Url::parse("scp://user:pass@[::1]/file.txt").unwrap();
    let target = ayurl::handlers::ssh_common::parse_ssh_url(&url).unwrap();
    assert_eq!(target.host, "::1");
    assert_eq!(target.username, "user");
    assert_eq!(target.password, Some("pass".to_string()));
}

#[tokio::test]
async fn ssh_parse_ipv6_no_user() {
    let url = url::Url::parse("scp://[::1]/file.txt").unwrap();
    let target = ayurl::handlers::ssh_common::parse_ssh_url(&url).unwrap();
    assert_eq!(target.host, "::1");
    assert!(!target.username.is_empty());
}

#[tokio::test]
async fn ssh_parse_ipv4_unchanged() {
    // IPv4 should pass through unchanged (no brackets to strip)
    let url = url::Url::parse("scp://user@192.168.1.1/file.txt").unwrap();
    let target = ayurl::handlers::ssh_common::parse_ssh_url(&url).unwrap();
    assert_eq!(target.host, "192.168.1.1");
}

#[tokio::test]
async fn ssh_parse_hostname_unchanged() {
    let url = url::Url::parse("scp://user@myhost.example.com/file.txt").unwrap();
    let target = ayurl::handlers::ssh_common::parse_ssh_url(&url).unwrap();
    assert_eq!(target.host, "myhost.example.com");
}

#[tokio::test]
async fn sftp_parse_ipv6() {
    let url = url::Url::parse("sftp://user@[fe80::1]:22/remote/file").unwrap();
    let target = ayurl::handlers::ssh_common::parse_ssh_url(&url).unwrap();
    assert_eq!(target.host, "fe80::1");
    assert_eq!(target.port, 22);
}

// --- HTTP IPv6 integration test ---

#[tokio::test]
async fn http_get_ipv6_localhost() {
    use axum::routing::get;
    use axum::Router;
    use tokio::net::TcpListener;

    let app = Router::new().route("/v6", get(|| async { "ipv6 works" }));
    let listener = TcpListener::bind("[::1]:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let text = ayurl::get(&format!("http://[::1]:{}/v6", addr.port()))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert_eq!(text, "ipv6 works");
}
