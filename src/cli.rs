use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use clap::{Parser, Subcommand};

use crate::error::Result;
use crate::progress::Progress;
use crate::scheme::{CredentialRequest, Credentials};

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

/// Prompt for a line of input on stderr (with echo).
/// Async-compatible: reads from stdin using tokio.
pub async fn prompt_line(prompt: &str) -> std::io::Result<String> {
    use std::io::Write;
    use tokio::io::AsyncBufReadExt;

    // Write prompt to stderr so it doesn't mix with data on stdout
    eprint!("{prompt}");
    std::io::stderr().flush()?;

    let mut line = String::new();
    let stdin = tokio::io::stdin();
    let mut reader = tokio::io::BufReader::new(stdin);
    reader.read_line(&mut line).await?;

    // Trim trailing newline
    if line.ends_with('\n') {
        line.pop();
        if line.ends_with('\r') {
            line.pop();
        }
    }

    Ok(line)
}

/// Prompt for a password on stderr (no echo).
/// Uses `rpassword` in a blocking task to avoid blocking the async runtime.
pub async fn prompt_password(prompt: &str) -> std::io::Result<String> {
    let prompt = prompt.to_string();
    tokio::task::spawn_blocking(move || rpassword::prompt_password(prompt))
        .await
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?
}

/// Create a credential callback that prompts the user interactively.
///
/// Returns a callback suitable for `on_credentials()` that:
/// 1. Prints the auth message to stderr
/// 2. Prompts for username (with echo, async via tokio stdin)
/// 3. Prompts for password (no echo, via rpassword in spawn_blocking)
///
/// Since the credential callback is synchronous (`Fn` not `async Fn`),
/// we use `tokio::runtime::Handle::block_on` inside spawn_blocking
/// for the async username prompt.
pub fn interactive_credential_callback(
) -> impl Fn(&CredentialRequest) -> Option<Credentials> + Send + Sync + 'static {
    move |req: &CredentialRequest| {
        eprintln!("{}", req.message);

        // We're called from an async context but the callback is sync.
        // Use the current tokio handle to run our async prompts.
        let handle = tokio::runtime::Handle::current();

        let username = std::thread::scope(|_| {
            handle.block_on(async {
                let url_username = req.url.username();
                if !url_username.is_empty() {
                    // Pre-fill from URL
                    eprintln!("Username [{}]: ", url_username);
                    let input = prompt_line("").await.ok()?;
                    if input.is_empty() {
                        Some(url_username.to_string())
                    } else {
                        Some(input)
                    }
                } else {
                    prompt_line("Username: ").await.ok()
                }
            })
        })?;

        let password = std::thread::scope(|_| {
            handle.block_on(async { prompt_password("Password: ").await.ok() })
        })?;

        // Handle multi-prompt (keyboard-interactive) auth
        if !req.prompts.is_empty() {
            let mut responses = Vec::new();
            for prompt in &req.prompts {
                let response = if prompt.echo {
                    std::thread::scope(|_| {
                        handle.block_on(async {
                            prompt_line(&prompt.message).await.ok()
                        })
                    })
                } else {
                    std::thread::scope(|_| {
                        handle.block_on(async {
                            prompt_password(&prompt.message).await.ok()
                        })
                    })
                };
                responses.push(response?);
            }
            return Some(Credentials {
                username: Some(username),
                secret: None,
                responses,
            });
        }

        Some(Credentials {
            username: Some(username),
            secret: Some(password),
            ..Default::default()
        })
    }
}

fn make_progress_callback() -> (impl Fn(&Progress) + Send + Sync + 'static, Arc<AtomicU64>) {
    let last_report = Arc::new(std::sync::Mutex::new(std::time::Instant::now()));
    let total_bytes = Arc::new(AtomicU64::new(0));
    let total_bytes_ret = total_bytes.clone();

    let cb = move |p: &Progress| {
        total_bytes.store(p.bytes_transferred, Ordering::Relaxed);
        let mut last = last_report.lock().unwrap();
        // Rate-limit output to at most every 100ms
        if last.elapsed() >= Duration::from_millis(100)
            || p.bytes_transferred == p.total_bytes.unwrap_or(0)
        {
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

    let mut get_req = crate::get(&src_uri);
    get_req = get_req.on_credentials(interactive_credential_callback());
    if progress {
        let (cb, _) = make_progress_callback();
        get_req = get_req.on_progress(cb);
    }

    let response = get_req.await?;
    let content_length = response.content_length();
    let reader = response.reader();

    let mut put_req = crate::put(&dst_uri).stream(reader);
    if let Some(len) = content_length {
        put_req = put_req.content_length(len);
    }
    put_req = put_req.on_credentials(interactive_credential_callback());

    let bytes_written = put_req.await?;

    if progress {
        eprintln!(); // newline after progress
    }

    Ok(bytes_written)
}

pub async fn run_get(uri: &str, progress: bool) -> Result<()> {
    let uri = normalize_uri(uri);
    tracing::info!(%uri, "get");

    let mut req = crate::get(&uri);
    req = req.on_credentials(interactive_credential_callback());
    if progress {
        let (cb, _) = make_progress_callback();
        req = req.on_progress(cb);
    }

    let response = req.await?;
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
    req = req.on_credentials(interactive_credential_callback());
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
