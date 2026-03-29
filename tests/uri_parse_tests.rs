/// Tests for ayurl's URI parser.

use ayurl::uri::ParsedUri;

// === Scheme extraction ===

#[test]
fn scheme_http() {
    let u = ParsedUri::parse("http://example.com/path").unwrap();
    assert_eq!(u.scheme(), "http");
}

#[test]
fn scheme_https() {
    let u = ParsedUri::parse("https://example.com/path").unwrap();
    assert_eq!(u.scheme(), "https");
}

#[test]
fn scheme_file() {
    let u = ParsedUri::parse("file:///tmp/foo").unwrap();
    assert_eq!(u.scheme(), "file");
}

#[test]
fn scheme_scp() {
    let u = ParsedUri::parse("scp://user@host/path").unwrap();
    assert_eq!(u.scheme(), "scp");
}

#[test]
fn scheme_sftp() {
    let u = ParsedUri::parse("sftp://user@host/path").unwrap();
    assert_eq!(u.scheme(), "sftp");
}

#[test]
fn scheme_custom() {
    let u = ParsedUri::parse("s3://bucket/key").unwrap();
    assert_eq!(u.scheme(), "s3");
}

#[test]
fn no_scheme_is_error() {
    assert!(ParsedUri::parse("just-a-path").is_err());
}

#[test]
fn empty_string_is_error() {
    assert!(ParsedUri::parse("").is_err());
}

// === Host extraction ===

#[test]
fn host_simple() {
    let u = ParsedUri::parse("scp://user@myhost.example.com/path").unwrap();
    assert_eq!(u.host(), Some("myhost.example.com"));
}

#[test]
fn host_ipv4() {
    let u = ParsedUri::parse("scp://user@192.168.1.1/path").unwrap();
    assert_eq!(u.host(), Some("192.168.1.1"));
}

#[test]
fn host_ipv6_loopback() {
    let u = ParsedUri::parse("scp://user@[::1]/path").unwrap();
    assert_eq!(u.host(), Some("::1"));
}

#[test]
fn host_ipv6_full() {
    let u = ParsedUri::parse("scp://user@[2a02:1811:1c88:7500:ceb7:d034:7ecb:4e30]//tmp/file")
        .unwrap();
    assert_eq!(
        u.host(),
        Some("2a02:1811:1c88:7500:ceb7:d034:7ecb:4e30")
    );
}

#[test]
fn host_ipv6_link_local() {
    let u = ParsedUri::parse("scp://user@[fe80::1]/path").unwrap();
    assert_eq!(u.host(), Some("fe80::1"));
}

#[test]
fn host_ipv6_with_port() {
    let u = ParsedUri::parse("scp://user@[::1]:2222/path").unwrap();
    assert_eq!(u.host(), Some("::1"));
    assert_eq!(u.port(), Some(2222));
}

#[test]
fn host_http_ipv6() {
    let u = ParsedUri::parse("http://[::1]:8080/path").unwrap();
    assert_eq!(u.host(), Some("::1"));
    assert_eq!(u.port(), Some(8080));
}

#[test]
fn host_file_is_none() {
    let u = ParsedUri::parse("file:///tmp/foo").unwrap();
    assert_eq!(u.host(), None);
}

#[test]
fn host_file_empty_is_none() {
    let u = ParsedUri::parse("file:///").unwrap();
    assert_eq!(u.host(), None);
}

// === Port ===

#[test]
fn port_explicit() {
    let u = ParsedUri::parse("scp://user@host:2222/path").unwrap();
    assert_eq!(u.port(), Some(2222));
}

#[test]
fn port_absent() {
    let u = ParsedUri::parse("scp://user@host/path").unwrap();
    assert_eq!(u.port(), None);
}

#[test]
fn port_ipv6_explicit() {
    let u = ParsedUri::parse("sftp://user@[::1]:22/path").unwrap();
    assert_eq!(u.port(), Some(22));
}

#[test]
fn port_ipv6_absent() {
    let u = ParsedUri::parse("sftp://user@[::1]/path").unwrap();
    assert_eq!(u.port(), None);
}

// === Username / Password ===

#[test]
fn username_only() {
    let u = ParsedUri::parse("scp://alice@host/path").unwrap();
    assert_eq!(u.username(), Some("alice"));
    assert_eq!(u.password(), None);
}

#[test]
fn username_and_password() {
    let u = ParsedUri::parse("scp://alice:s3cret@host/path").unwrap();
    assert_eq!(u.username(), Some("alice"));
    assert_eq!(u.password(), Some("s3cret"));
}

#[test]
fn no_userinfo() {
    let u = ParsedUri::parse("scp://host/path").unwrap();
    assert_eq!(u.username(), None);
    assert_eq!(u.password(), None);
}

#[test]
fn username_empty_password() {
    let u = ParsedUri::parse("scp://alice:@host/path").unwrap();
    assert_eq!(u.username(), Some("alice"));
    assert_eq!(u.password(), Some(""));
}

#[test]
fn password_with_special_chars() {
    let u = ParsedUri::parse("scp://alice:p%40ss%3Aword@host/path").unwrap();
    assert_eq!(u.username(), Some("alice"));
    assert_eq!(u.password(), Some("p@ss:word"));
}

#[test]
fn username_with_special_chars() {
    let u = ParsedUri::parse("scp://al%40ice:pass@host/path").unwrap();
    assert_eq!(u.username(), Some("al@ice"));
}

#[test]
fn http_userinfo() {
    let u = ParsedUri::parse("http://user:pass@example.com/path").unwrap();
    assert_eq!(u.username(), Some("user"));
    assert_eq!(u.password(), Some("pass"));
}

// === Path ===

#[test]
fn path_simple() {
    let u = ParsedUri::parse("scp://user@host/remote/file.txt").unwrap();
    assert_eq!(u.path(), "/remote/file.txt");
}

#[test]
fn path_absolute_double_slash() {
    // scp://host//absolute/path — double slash means absolute path on remote
    let u = ParsedUri::parse("scp://user@host//tmp/file").unwrap();
    assert_eq!(u.path(), "//tmp/file");
}

#[test]
fn path_file_uri() {
    let u = ParsedUri::parse("file:///tmp/foo.txt").unwrap();
    assert_eq!(u.path(), "/tmp/foo.txt");
}

#[test]
fn path_http() {
    let u = ParsedUri::parse("http://example.com/api/v1/data").unwrap();
    assert_eq!(u.path(), "/api/v1/data");
}

#[test]
fn path_with_query() {
    let u = ParsedUri::parse("http://example.com/path?key=value").unwrap();
    assert_eq!(u.path(), "/path");
    assert_eq!(u.query(), Some("key=value"));
}

#[test]
fn path_with_fragment() {
    let u = ParsedUri::parse("http://example.com/path#section").unwrap();
    assert_eq!(u.path(), "/path");
    assert_eq!(u.fragment(), Some("section"));
}

#[test]
fn path_root() {
    let u = ParsedUri::parse("http://example.com/").unwrap();
    assert_eq!(u.path(), "/");
}

#[test]
fn path_empty_becomes_slash() {
    let u = ParsedUri::parse("http://example.com").unwrap();
    assert_eq!(u.path(), "/");
}

// === Query and Fragment ===

#[test]
fn query_string() {
    let u = ParsedUri::parse("http://example.com/path?foo=bar&baz=1").unwrap();
    assert_eq!(u.query(), Some("foo=bar&baz=1"));
}

#[test]
fn no_query() {
    let u = ParsedUri::parse("http://example.com/path").unwrap();
    assert_eq!(u.query(), None);
}

#[test]
fn fragment_only() {
    let u = ParsedUri::parse("http://example.com/path#frag").unwrap();
    assert_eq!(u.fragment(), Some("frag"));
    assert_eq!(u.query(), None);
}

#[test]
fn query_and_fragment() {
    let u = ParsedUri::parse("http://example.com/path?q=1#frag").unwrap();
    assert_eq!(u.query(), Some("q=1"));
    assert_eq!(u.fragment(), Some("frag"));
}

// === Full URI reconstruction ===

#[test]
fn to_string_roundtrip_http() {
    let input = "http://user:pass@example.com:8080/path?q=1#frag";
    let u = ParsedUri::parse(input).unwrap();
    assert_eq!(u.scheme(), "http");
    assert_eq!(u.username(), Some("user"));
    assert_eq!(u.password(), Some("pass"));
    assert_eq!(u.host(), Some("example.com"));
    assert_eq!(u.port(), Some(8080));
    assert_eq!(u.path(), "/path");
    assert_eq!(u.query(), Some("q=1"));
    assert_eq!(u.fragment(), Some("frag"));
}

#[test]
fn to_string_roundtrip_scp_ipv6() {
    let input = "scp://user:pass@[2a02:1811::1]:2222//tmp/file";
    let u = ParsedUri::parse(input).unwrap();
    assert_eq!(u.scheme(), "scp");
    assert_eq!(u.username(), Some("user"));
    assert_eq!(u.password(), Some("pass"));
    assert_eq!(u.host(), Some("2a02:1811::1"));
    assert_eq!(u.port(), Some(2222));
    assert_eq!(u.path(), "//tmp/file");
}

// === Edge cases ===

#[test]
fn scheme_with_plus_and_dot() {
    let u = ParsedUri::parse("remote+http://host/path").unwrap();
    assert_eq!(u.scheme(), "remote+http");
}

#[test]
fn ipv6_no_userinfo_no_port() {
    let u = ParsedUri::parse("scp://[::1]/file").unwrap();
    assert_eq!(u.host(), Some("::1"));
    assert_eq!(u.username(), None);
    assert_eq!(u.port(), None);
}

#[test]
fn file_uri_windows_style() {
    // file:///C:/Users/foo — Windows path
    let u = ParsedUri::parse("file:///C:/Users/foo").unwrap();
    assert_eq!(u.scheme(), "file");
    assert_eq!(u.path(), "/C:/Users/foo");
}

// === RFC 8089 compliance ===

#[test]
fn file_uri_localhost_authority_stripped() {
    // RFC 8089 §2: "localhost" authority means local machine
    let u = ParsedUri::parse("file://localhost/tmp/foo").unwrap();
    assert_eq!(u.scheme(), "file");
    assert_eq!(u.host(), None);
    assert_eq!(u.path(), "/tmp/foo");
}

#[test]
fn file_uri_localhost_case_insensitive() {
    let u = ParsedUri::parse("file://LOCALHOST/tmp/foo").unwrap();
    assert_eq!(u.path(), "/tmp/foo");
    assert_eq!(u.host(), None);
}

#[test]
fn file_uri_percent_encoded_space() {
    // RFC 8089 inherits RFC 3986 percent-encoding
    let u = ParsedUri::parse("file:///tmp/my%20file.txt").unwrap();
    assert_eq!(u.path(), "/tmp/my file.txt");
}

#[test]
fn file_uri_percent_encoded_special_chars() {
    let u = ParsedUri::parse("file:///path/%23hash%3Fquery").unwrap();
    assert_eq!(u.path(), "/path/#hash?query");
}

#[test]
fn file_uri_query_parsed() {
    let u = ParsedUri::parse("file:///tmp/foo?key=value").unwrap();
    assert_eq!(u.path(), "/tmp/foo");
    assert_eq!(u.query(), Some("key=value"));
}

#[test]
fn file_uri_fragment_parsed() {
    let u = ParsedUri::parse("file:///tmp/foo#section1").unwrap();
    assert_eq!(u.path(), "/tmp/foo");
    assert_eq!(u.fragment(), Some("section1"));
}

#[test]
fn file_uri_query_and_fragment_parsed() {
    let u = ParsedUri::parse("file:///tmp/foo?q=1#frag").unwrap();
    assert_eq!(u.path(), "/tmp/foo");
    assert_eq!(u.query(), Some("q=1"));
    assert_eq!(u.fragment(), Some("frag"));
}

#[test]
fn file_uri_dot_authority_relative_to_cwd() {
    // Non-standard but browser-supported: file://./local/path
    // Should resolve relative to current directory
    let u = ParsedUri::parse("file://./local/path").unwrap();
    assert_eq!(u.scheme(), "file");
    let cwd = std::env::current_dir().unwrap();
    let expected = format!("{}/local/path", cwd.display());
    assert_eq!(u.path(), expected);
}
