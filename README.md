# ayurl

Async URI-based data transfer library and CLI for Rust. Get and put data regardless of location — `file://`, `http://`, `https://`, `scp://`, `sftp://`, and custom schemes.

## Quick Start

### As a library

```rust
use std::time::Duration;

#[tokio::main]
async fn main() -> ayurl::Result<()> {
    // One-shot fetch
    let data = ayurl::get("https://example.com/data.json").await?.bytes().await?;
    let text = ayurl::get("file:///etc/hostname").await?.text().await?;

    // One-shot put
    ayurl::put("file:///tmp/output.txt").text("hello world").await?;

    // Streaming (Response implements futures::io::AsyncRead)
    let mut response = ayurl::get("https://example.com/big.bin").await?;
    // use response as any AsyncRead...

    // With progress reporting
    let data = ayurl::get("scp://user:pass@host/path/file.bin")
        .on_progress(|p| {
            eprintln!("{}/{} bytes", p.bytes_transferred, p.total_bytes.unwrap_or(0));
        })
        .await?
        .bytes()
        .await?;

    // Copy between any URIs
    let response = ayurl::get("https://example.com/archive.tar.gz").await?;
    let len = response.content_length();
    let mut req = ayurl::put("scp://user@host//tmp/archive.tar.gz").stream(response);
    if let Some(len) = len {
        req = req.content_length(len); // enables streaming SCP upload
    }
    req.await?;

    Ok(())
}
```

### As a CLI tool

```bash
# Copy between any URIs (file, http, scp, sftp)
ayurl copy https://example.com/file.bin /tmp/local.bin
ayurl cp scp://user@host/remote/file.bin ./local.bin
ayurl cp ./local.bin scp://user@remotehost//tmp/remote.bin

# Fetch to stdout
ayurl get https://example.com/api/data
ayurl cat /etc/hostname

# Write stdin to a URI
echo "hello" | ayurl put scp://user@host/greeting.txt

# Progress indicator (-p for frequent updates, default: every 2s)
ayurl copy -p https://example.com/big.iso /tmp/big.iso
```

Bare file paths are automatically promoted to `file://` URIs. IPv6 addresses are supported: `scp://user@[::1]/path`.

## Features

### Scheme Handlers

| Scheme | Feature flag | Backend | Auth | Streaming |
|--------|-------------|---------|------|-----------|
| `file://` | `file` | tokio::fs | N/A | Full |
| `http://`, `https://` | `http` | reqwest | Basic auth, Bearer token, credential callback | Full |
| `scp://` | `scp` | ayssh | Password, public key, credential callback | Download: full; Upload: with content_length hint |
| `sftp://` | `sftp` | ayssh | Password, public key, credential callback | Upload: full; Download: buffered |

All features are enabled by default.

### Custom Scheme Handlers

Register handlers at build time for any URI scheme:

```rust
use async_trait::async_trait;
use ayurl::{SchemeHandler, ParsedUri, TransferContext};
use futures::io::AsyncRead;

struct S3Handler { /* ... */ }

#[async_trait]
impl SchemeHandler for S3Handler {
    async fn get(&self, uri: &ParsedUri, ctx: &mut TransferContext)
        -> ayurl::Result<Box<dyn AsyncRead + Send + Unpin>> {
        // implement S3 download...
        # todo!()
    }
    async fn put(&self, uri: &ParsedUri, body: Box<dyn AsyncRead + Send + Unpin>,
        ctx: &mut TransferContext) -> ayurl::Result<u64> {
        // implement S3 upload...
        # todo!()
    }
}

let client = ayurl::Client::builder()
    .register_scheme("s3", S3Handler { /* ... */ })
    .build();

let data = client.get("s3://bucket/key").await?.bytes().await?;
```

### Client Configuration

```rust
use std::time::Duration;

// Module-level functions use a lazy global default client
ayurl::get("file:///tmp/foo").await?;

// Configure the global default (must be called before first use)
ayurl::configure_default(|builder| {
    builder.timeout(Duration::from_secs(60))
})?;

// Or create explicit clients for full control
let client = ayurl::Client::builder()
    .timeout(Duration::from_secs(30))
    .connector(my_tunnel)           // transport-level tunneling
    .on_credentials(|req| { ... })  // credential callback
    .register_scheme("s3", s3_handler)
    .build();
```

### Credential Handling

Authentication is reactive — handlers try the request, and on auth failure (HTTP 401, SSH auth reject), they call the credential callback.

**Resolution order:**
1. Credentials from the URL (`scp://user:pass@host/path`)
2. Scheme-specific options (`HttpOptions::bearer_token()`, `SshOptions::with_private_key_path()`)
3. Credential callback (if set on Client or per-request)

```rust
// URL credentials
ayurl::get("http://alice:secret@example.com/api").await?;

// SSH with private key
ayurl::get("sftp://user@host/path")
    .with_options(ayurl::SshOptions::new()
        .with_private_key_path("/home/user/.ssh/id_ed25519"))
    .await?;

// HTTP with custom headers
ayurl::get("https://api.example.com/data")
    .with_options(ayurl::HttpOptions::new()
        .bearer_token("my-token")
        .header("X-Custom", "value"))
    .await?;

// Client-level credential callback
let client = ayurl::Client::builder()
    .on_credentials(|req| {
        eprintln!("{}", req.message); // "Authentication required for host"
        Some(ayurl::Credentials {
            username: Some("user".into()),
            secret: Some(read_password()),
            ..Default::default()
        })
    })
    .build();

// Per-request override
client.get("http://example.com/protected")
    .on_credentials(|req| { /* ... */ None })
    .await?;
```

### Response Consumers

```rust
let response = ayurl::get("https://example.com/file").await?;

// Streaming (default) — Response implements futures::io::AsyncRead
let mut reader = response;
futures::io::copy(&mut reader, &mut output).await?;

// One-shot consumers
let bytes: Vec<u8> = response.bytes().await?;
let text: String = response.text().await?;

// Lossy variants — never fail, return what was received
let bytes: Vec<u8> = response.bytes_lossy().await;
let text: String = response.text_lossy().await;  // replaces invalid UTF-8 with U+FFFD

// Reader variants
let reader = response.reader();           // propagates errors
let reader = response.lenient_reader();   // returns EOF on error
```

### Progress Reporting

```rust
// Callback
let data = ayurl::get("https://example.com/big.bin")
    .on_progress(|p| {
        eprintln!("{}/{} bytes ({:.1}s)",
            p.bytes_transferred,
            p.total_bytes.unwrap_or(0),
            p.elapsed.as_secs_f64());
    })
    .await?
    .bytes()
    .await?;

// Watch channel (for async UI updates)
let (req, mut rx) = ayurl::get("https://example.com/big.bin").progress_channel();

tokio::spawn(async move {
    while rx.changed().await.is_ok() {
        let p = rx.borrow();
        update_progress_bar(p.bytes_transferred, p.total_bytes);
    }
});

let response = req.await?;
```

### Transport Tunneling

The `Connector` trait abstracts transport-level connections. Scheme handlers use it instead of connecting directly, enabling transparent tunneling:

```rust
use ayurl::{Connector, AsyncReadWrite};

struct SocksConnector { proxy_addr: SocketAddr }

#[async_trait]
impl Connector for SocksConnector {
    async fn connect(&self, host: &str, port: u16)
        -> ayurl::Result<Box<dyn AsyncReadWrite + Send + Unpin>> {
        // connect through SOCKS proxy...
        # todo!()
    }
}

let client = ayurl::Client::builder()
    .connector(SocksConnector { proxy_addr: "127.0.0.1:1080".parse().unwrap() })
    .build();

// All HTTP requests now go through the SOCKS proxy
client.get("https://example.com/data").await?;
```

## Architecture

```
ayurl::Client
  ├── SchemeHandler registry (file, http, scp, sftp, custom...)
  ├── Connector (DirectConnector, or custom for tunneling)
  ├── CredentialCallback (reactive auth on failure)
  └── Default timeout

ayurl::get(uri) / ayurl::put(uri)
  └── GetRequest / PutRequest (builder, impl IntoFuture)
        ├── .on_progress(callback)
        ├── .on_credentials(callback)
        ├── .with_options(scheme_specific)
        ├── .timeout(duration)
        ├── .content_length(hint)    // enables streaming SCP upload
        └── .await → Response (impl AsyncRead)
```

## Module Layout

```
src/
├── lib.rs          Public API, re-exports, module-level get/put
├── uri.rs          Custom URI parser (consistent IPv6, all schemes)
├── client.rs       Client, ClientBuilder, scheme registry, global default
├── request.rs      GetRequest/PutRequest builders with IntoFuture
├── response.rs     Response (AsyncRead, bytes/text/lossy consumers)
├── progress.rs     Progress tracking, ProgressReader wrapper
├── error.rs        AyurlError, Result type alias
├── scheme.rs       SchemeHandler trait, Connector, credentials types
├── cli.rs          CLI (clap): copy/get/put commands, interactive prompts
├── main.rs         Binary entry point
└── handlers/
    ├── file.rs     file:// handler
    ├── http.rs     http/https handler (reqwest) with HttpOptions
    ├── scp.rs      scp:// handler (ayssh) with streaming download
    ├── sftp.rs     sftp:// handler (ayssh) with streaming upload
    └── ssh_common.rs  Shared SSH types, SshOptions, adapters
```

## Dependencies

Core: `tokio`, `futures`, `thiserror`, `async-trait`, `tracing`, `pin-project-lite`, `tokio-util`

Optional:
- `reqwest` — HTTP/HTTPS support (feature: `http`)
- `ayssh` — SCP/SFTP support (features: `scp`, `sftp`)

CLI: `clap`, `rpassword`, `tracing-subscriber`

## Testing

```bash
cargo test                        # 189 tests, all schemes
cargo test --test scp_sftp_tests  # SCP/SFTP with in-process SSH server
cargo test --test pty_tests       # interactive credential prompts via PTY
cargo test --test uri_parse_tests # URI parser (45 tests)

RUST_LOG=ayurl=debug cargo run -- copy scp://user@host/file ./local  # debug logging
```

Test infrastructure:
- HTTP tests use `axum` as an in-process test server
- SCP/SFTP tests use `ayssh::server::test_server::TestSshServer` (no external sshd needed)
- PTY tests use `expectrl` for interactive terminal testing
- Test SSH key pair at `tests/fixtures/test_key_ed25519`

## License

MIT
