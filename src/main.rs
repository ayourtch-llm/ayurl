use clap::Parser;

#[tokio::main]
async fn main() -> ayurl::Result<()> {
    ayurl::init_tracing();

    let cli = ayurl::cli::Cli::parse();

    match cli.command {
        ayurl::cli::Command::Copy { src, dst, progress } => {
            let bytes = ayurl::cli::run_copy(&src, &dst, progress).await?;
            eprintln!("Copied {bytes} bytes");
        }
        ayurl::cli::Command::Get { uri, progress } => {
            ayurl::cli::run_get(&uri, progress).await?;
        }
        ayurl::cli::Command::Put { uri, progress } => {
            ayurl::cli::run_put(&uri, progress).await?;
        }
    }

    Ok(())
}
