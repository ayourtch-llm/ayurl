//! # ayurl
//!
//! Async URI-based data transfer library. Get and put data regardless of
//! location — `file://`, `http://`, `https://`, `scp://`, and custom schemes.
//!
//! ## Quick Start
//!
//! ```no_run
//! # async fn example() -> ayurl::Result<()> {
//! // One-shot get
//! let data = ayurl::get("file:///tmp/hello.txt").await?.text().await?;
//!
//! // One-shot put
//! ayurl::put("file:///tmp/output.txt").text("hello world").await?;
//!
//! // Streaming
//! let mut reader = ayurl::get("https://example.com/big.bin").await?;
//! // reader implements futures::io::AsyncRead
//!
//! // With progress
//! let data = ayurl::get("https://example.com/big.bin")
//!     .on_progress(|p| eprintln!("{} bytes", p.bytes_transferred))
//!     .await?
//!     .bytes()
//!     .await?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Custom Client
//!
//! ```no_run
//! use std::time::Duration;
//!
//! # async fn example() -> ayurl::Result<()> {
//! let client = ayurl::Client::builder()
//!     .timeout(Duration::from_secs(30))
//!     .build();
//!
//! let data = client.get("https://example.com/api").await?.bytes().await?;
//! # Ok(())
//! # }
//! ```

pub mod cli;
pub mod client;
pub mod error;
pub mod handlers;
pub mod progress;
pub mod request;
pub mod response;
pub mod scheme;

// Re-exports for convenience
pub use client::{configure_default, Client, ClientBuilder};
pub use error::{AyurlError, Result};
pub use progress::Progress;
pub use request::{GetRequest, PutRequest};
pub use response::{LenientReader, Response};
pub use scheme::{
    AsyncReadWrite, Connector, CredentialCallback, CredentialKind, CredentialRequest, Credentials,
    DirectConnector, SchemeCapabilities, SchemeHandler, TransferContext,
};

#[cfg(feature = "http")]
pub use handlers::http::HttpOptions;

/// Start a GET request using the global default client.
///
/// Returns a `GetRequest` builder that implements `IntoFuture` — you can
/// `.await` it directly or chain configuration methods first.
pub fn get(uri: &str) -> GetRequest {
    client::default_client().get(uri)
}

/// Start a PUT request using the global default client.
///
/// Returns a `PutRequest` builder that implements `IntoFuture` — you can
/// `.await` it directly or chain configuration methods first.
pub fn put(uri: &str) -> PutRequest {
    client::default_client().put(uri)
}

/// Initialize tracing/logging with environment filter.
/// Reads `RUST_LOG` env var, defaults to `info` level.
pub fn init_tracing() {
    use tracing_subscriber::EnvFilter;
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();
}
