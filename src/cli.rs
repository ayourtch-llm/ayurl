use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use clap::{Parser, Subcommand};

use crate::error::Result;
use crate::progress::Progress;

#[derive(Parser)]
#[command(name = "ayurl", about = "URI-based data transfer tool")]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Copy data from one URI to another
    #[command(alias = "cp")]
    Copy {
        /// Source URI (or local path)
        src: String,
        /// Destination URI (or local path)
        dst: String,
        /// Show progress indicator
        #[arg(short, long)]
        progress: bool,
    },
    /// Fetch a URI and print to stdout
    #[command(alias = "cat")]
    Get {
        /// URI to fetch (or local path)
        uri: String,
        /// Show progress indicator on stderr
        #[arg(short, long)]
        progress: bool,
    },
    /// Write stdin to a URI
    Put {
        /// Destination URI (or local path)
        uri: String,
        /// Show progress indicator on stderr
        #[arg(short, long)]
        progress: bool,
    },
}

/// Normalize a string to a URI. If it doesn't have a scheme,
/// treat it as a local file path and produce a file:// URI.
pub fn normalize_uri(s: &str) -> String {
    // Already has a scheme?
    if s.contains("://") {
        return s.to_string();
    }

    // Treat as file path
    let path = Path::new(s);
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_default()
            .join(path)
    };
    format!("file://{}", abs.display())
}

fn make_progress_callback() -> (impl Fn(&Progress) + Send + Sync + 'static, Arc<AtomicU64>) {
    let last_report = Arc::new(std::sync::Mutex::new(std::time::Instant::now()));
    let total_bytes = Arc::new(AtomicU64::new(0));
    let total_bytes_ret = total_bytes.clone();

    let cb = move |p: &Progress| {
        total_bytes.store(p.bytes_transferred, Ordering::Relaxed);
        let mut last = last_report.lock().unwrap();
        // Rate-limit output to at most every 100ms
        if last.elapsed() >= Duration::from_millis(100) || p.bytes_transferred == p.total_bytes.unwrap_or(0) {
            *last = std::time::Instant::now();
            if let Some(total) = p.total_bytes {
                if total > 0 {
                    let pct = (p.bytes_transferred as f64 / total as f64) * 100.0;
                    eprint!(
                        "\r[{}/{} bytes  {:.0}%  {:.1}s]",
                        p.bytes_transferred,
                        total,
                        pct,
                        p.elapsed.as_secs_f64()
                    );
                } else {
                    eprint!(
                        "\r[{} bytes  {:.1}s]",
                        p.bytes_transferred,
                        p.elapsed.as_secs_f64()
                    );
                }
            } else {
                eprint!(
                    "\r[{} bytes  {:.1}s]",
                    p.bytes_transferred,
                    p.elapsed.as_secs_f64()
                );
            }
        }
    };

    (cb, total_bytes_ret)
}

pub async fn run_copy(src: &str, dst: &str, progress: bool) -> Result<u64> {
    let src_uri = normalize_uri(src);
    let dst_uri = normalize_uri(dst);

    tracing::info!(%src_uri, %dst_uri, "copy");

    let response = if progress {
        let (cb, _) = make_progress_callback();
        crate::get(&src_uri).on_progress(cb).await?
    } else {
        crate::get(&src_uri).await?
    };

    let reader = response.reader();
    let bytes_written = crate::put(&dst_uri).stream(reader).await?;

    if progress {
        eprintln!(); // newline after progress
    }

    Ok(bytes_written)
}

pub async fn run_get(uri: &str, progress: bool) -> Result<()> {
    let uri = normalize_uri(uri);
    tracing::info!(%uri, "get");

    let response = if progress {
        let (cb, _) = make_progress_callback();
        crate::get(&uri).on_progress(cb).await?
    } else {
        crate::get(&uri).await?
    };

    let data = response.bytes_lossy().await;

    if progress {
        eprintln!(); // newline after progress
    }

    // Write raw bytes to stdout
    use std::io::Write;
    std::io::stdout().write_all(&data)?;
    std::io::stdout().flush()?;

    Ok(())
}

pub async fn run_put(uri: &str, progress: bool) -> Result<()> {
    let uri = normalize_uri(uri);
    tracing::info!(%uri, "put");

    // Read stdin into memory (streaming stdin is possible but complex)
    let mut buf = Vec::new();
    use tokio::io::AsyncReadExt;
    tokio::io::stdin().read_to_end(&mut buf).await?;

    let mut req = crate::put(&uri).bytes(buf);
    if progress {
        let (cb, _) = make_progress_callback();
        req = req.on_progress(cb);
    }

    let bytes_written = req.await?;

    if progress {
        eprintln!();
    }

    eprintln!("Wrote {bytes_written} bytes");
    Ok(())
}
