use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use herd_hub::{Hub, MemoryJobStore, DEFAULT_LISTEN_ADDR};

/// The Herd hub: accepts a runner and requesters, dispatches jobs, relays logs.
#[derive(Debug, Parser)]
#[command(name = "herd-hub", version, about)]
struct Args {
    /// Address to listen on. Plain ws: TLS is the reverse proxy's job (D4).
    #[arg(long, env = "HERD_LISTEN", default_value = DEFAULT_LISTEN_ADDR)]
    listen: SocketAddr,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    // M2 swaps this one line for the on-disk store (D8).
    let hub = Hub::bind(args.listen, Arc::new(MemoryJobStore::new())).await?;
    println!(
        "herd-hub {} listening on ws://{}",
        herd_hub::hub_version(),
        hub.local_addr()
    );
    hub.serve().await
}
