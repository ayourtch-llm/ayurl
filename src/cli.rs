use std::path::Path;

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

/// Prompt for a line of input synchronously (with echo).
/// Uses stderr for the prompt so stdout stays clean for data.
fn prompt_line_sync(prompt: &str) -> std::io::Result<String> {
    use std::io::Write;
    eprint!("{prompt}");
    std::io::stderr().flush()?;

    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;

    if line.ends_with('\n') {
        line.pop();
        if line.ends_with('\r') {
            line.pop();
        }
    }
    Ok(line)
}

/// Create a credential callback that prompts the user interactively.
///
/// Returns a callback suitable for `on_credentials()` that:
/// 1. Prints the auth message to stderr
/// 2. Prompts for username (with echo, sync stdin)
/// 3. Prompts for password (no echo, via rpassword)
///
/// Uses blocking I/O since credential prompts are inherently interactive
/// and brief. This avoids the "cannot block_on inside a runtime" problem.
pub fn interactive_credential_callback(
) -> impl Fn(&CredentialRequest) -> Option<Credentials> + Send + Sync + 'static {
    move |req: &CredentialRequest| {
        eprintln!("{}", req.message);

        // Handle multi-prompt (keyboard-interactive) auth
        if !req.prompts.is_empty() {
            let url_username = req.uri.username().unwrap_or("");
            let username = if !url_username.is_empty() {
                url_username.to_string()
            } else {
                prompt_line_sync("Username: ").ok()?
            };

            let mut responses = Vec::new();
            for prompt in &req.prompts {
                let response = if prompt.echo {
                    prompt_line_sync(&prompt.message).ok()?
                } else {
                    rpassword::prompt_password(&prompt.message).ok()?
                };
                responses.push(response);
            }
            return Some(Credentials {
                username: Some(username),
                secret: None,
                responses,
            });
        }

        // Standard username/password auth
        let url_username = req.uri.username().unwrap_or("");
        let username = if !url_username.is_empty() {
            let input = prompt_line_sync(&format!("Username [{}]: ", url_username)).ok()?;
            if input.is_empty() {
                url_username.to_string()
            } else {
                input
            }
        } else {
            prompt_line_sync("Username: ").ok()?
        };

        let password = rpassword::prompt_password("Password: ").ok()?;

        Some(Credentials {
            username: Some(username),
            secret: Some(password),
            ..Default::default()
        })
    }
}

/// Format bytes as a human-readable size string.
fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}

/// Build a progress callback with the given reporting interval.
fn make_progress_callback(
    interval: Duration,
) -> impl Fn(&Progress) + Send + Sync + 'static {
    let last_report = Arc::new(std::sync::Mutex::new(std::time::Instant::now()));

    move |p: &Progress| {
        let mut last = last_report.lock().unwrap();
        let is_done = p.total_bytes.is_some_and(|t| t > 0 && p.bytes_transferred >= t);
        if last.elapsed() >= interval || is_done {
            *last = std::time::Instant::now();
            let transferred = format_bytes(p.bytes_transferred);
            // Clear line then write progress — avoids garbled output from
            // shorter lines not fully overwriting longer ones.
            let line = if let Some(total) = p.total_bytes {
                if total > 0 {
                    let pct = (p.bytes_transferred as f64 / total as f64) * 100.0;
                    let total_str = format_bytes(total);
                    format!(
                        "  {transferred} / {total_str}  ({pct:.0}%)  {:.1}s",
                        p.elapsed.as_secs_f64()
                    )
                } else {
                    format!(
                        "  {transferred}  {:.1}s",
                        p.elapsed.as_secs_f64()
                    )
                }
            } else {
                format!(
                    "  {transferred}  {:.1}s",
                    p.elapsed.as_secs_f64()
                )
            };
            eprint!("\r\x1b[2K{line}");
        }
    }
}

pub async fn run_copy(src: &str, dst: &str, progress: bool) -> Result<u64> {
    let src_uri = normalize_uri(src);
    let dst_uri = normalize_uri(dst);

    let interval = if progress {
        Duration::from_millis(100)
    } else {
        Duration::from_secs(2)
    };

    eprintln!("Fetching {src_uri} ...");

    let get_req = crate::get(&src_uri)
        .on_credentials(interactive_credential_callback())
        .on_progress(make_progress_callback(interval));

    let response = get_req.await?;
    let content_length = response.content_length();

    if let Some(len) = content_length {
        eprintln!("\rSource: {}", format_bytes(len));
    }

    let reader = response.reader();

    eprintln!("Sending to {dst_uri} ...");

    let mut put_req = crate::put(&dst_uri)
        .stream(reader)
        .on_credentials(interactive_credential_callback())
        .on_progress(make_progress_callback(interval));
    if let Some(len) = content_length {
        put_req = put_req.content_length(len);
    }

    let bytes_written = put_req.await?;
    eprintln!(); // newline after progress

    Ok(bytes_written)
}

pub async fn run_get(uri: &str, progress: bool) -> Result<()> {
    let uri = normalize_uri(uri);
    eprintln!("Fetching {uri} ...");

    let interval = if progress {
        Duration::from_millis(100)
    } else {
        Duration::from_secs(2)
    };

    let req = crate::get(&uri)
        .on_credentials(interactive_credential_callback())
        .on_progress(make_progress_callback(interval));

    let response = req.await?;
    let data = response.bytes_lossy().await;
    eprintln!(); // newline after progress

    // Write raw bytes to stdout
    use std::io::Write;
    std::io::stdout().write_all(&data)?;
    std::io::stdout().flush()?;

    Ok(())
}

pub async fn run_put(uri: &str, progress: bool) -> Result<()> {
    let uri = normalize_uri(uri);
    eprintln!("Reading stdin ...");

    // Read stdin into memory (streaming stdin is possible but complex)
    let mut buf = Vec::new();
    use tokio::io::AsyncReadExt;
    tokio::io::stdin().read_to_end(&mut buf).await?;

    let interval = if progress {
        Duration::from_millis(100)
    } else {
        Duration::from_secs(2)
    };

    eprintln!("Sending to {uri} ...");

    let req = crate::put(&uri)
        .bytes(buf)
        .on_credentials(interactive_credential_callback())
        .on_progress(make_progress_callback(interval));

    let bytes_written = req.await?;
    eprintln!();

    eprintln!("Wrote {}", format_bytes(bytes_written));
    Ok(())
}
