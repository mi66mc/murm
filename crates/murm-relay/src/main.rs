use clap::Parser;
use murm_relay::{RelayConfig, RelayService, SystemClock, serve};
use std::{path::PathBuf, sync::Arc};
#[derive(Parser)]
#[command(
    version,
    about = "Experimental Murm publication relay (loopback; deploy behind HTTPS)"
)]
struct Args {
    #[arg(long)]
    config: Option<PathBuf>,
    #[arg(long)]
    db: Option<PathBuf>,
    #[arg(long)]
    listen: Option<std::net::SocketAddr>,
}
#[tokio::main(worker_threads = 2)]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let mut config = if let Some(path) = args.config {
        RelayConfig::load(&path)?
    } else {
        RelayConfig::default()
    };
    if let Some(db) = args.db {
        config.database_path = db;
    }
    if let Some(listen) = args.listen {
        config.listen = listen;
    }
    config.validate()?;
    let listener = tokio::net::TcpListener::bind(config.listen).await?;
    let address = listener.local_addr()?;
    let service = Arc::new(RelayService::open(config, Arc::new(SystemClock))?);
    eprintln!("Murm relay listening on http://{address}");
    let replicator = tokio::spawn(murm_relay::Replicator::new(service.clone())?.run());
    let result = serve(listener, service, async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await;
    replicator.abort();
    result?;
    Ok(())
}
