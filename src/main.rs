use anyhow::Result;
use clap::Parser;
use spg::{app, cli::Cli};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    app::run(cli).await
}
