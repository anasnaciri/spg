use anyhow::Result;
use clap::Parser;
use spg::cli::{CacheCommand, Cli, Commands, ConfigCommand};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    run(cli).await
}

async fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Commands::Init(args) => {
            let name = args.project_name.as_deref().unwrap_or("<prompt>");
            eprintln!("spg init is not implemented yet for {name}");
        }
        Commands::Deps => {
            eprintln!("spg deps is not implemented yet");
        }
        Commands::Config(ConfigCommand::Show) => {
            eprintln!("spg config show is not implemented yet");
        }
        Commands::Config(ConfigCommand::Reset) => {
            eprintln!("spg config reset is not implemented yet");
        }
        Commands::Cache(CacheCommand::Clear) => {
            eprintln!("spg cache clear is not implemented yet");
        }
    }

    Ok(())
}
