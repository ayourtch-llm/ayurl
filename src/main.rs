use std::env;

#[tokio::main]
async fn main() -> ayurl::Result<()> {
    ayurl::init_tracing();

    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: ayurl <URI> [output-file]");
        eprintln!("  ayurl file:///etc/hostname");
        eprintln!("  ayurl https://example.com/");
        eprintln!("  ayurl https://example.com/ /tmp/output.html");
        std::process::exit(1);
    }

    let uri = &args[1];

    let response = ayurl::get(uri)
        .on_progress(|p| {
            if let Some(total) = p.total_bytes {
                eprintln!("[{}/{}  {:.1}s]", p.bytes_transferred, total, p.elapsed.as_secs_f64());
            } else {
                eprintln!("[{}  {:.1}s]", p.bytes_transferred, p.elapsed.as_secs_f64());
            }
        })
        .await?;

    if let Some(output_path) = args.get(2) {
        // Write to file
        let output_uri = if output_path.starts_with("file://") {
            output_path.to_string()
        } else {
            format!("file://{}", std::path::Path::new(output_path).canonicalize().unwrap_or_else(|_| std::path::PathBuf::from(output_path)).display())
        };
        let data = response.bytes().await?;
        let bytes_written = ayurl::put(&output_uri).bytes(data).await?;
        eprintln!("Wrote {bytes_written} bytes to {output_path}");
    } else {
        // Print to stdout
        let text = response.text_lossy().await;
        print!("{text}");
    }

    Ok(())
}
