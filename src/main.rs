use clap::Parser;
use jj_vine::{cli::Cli, error::Result};

#[tokio::main]
async fn main() -> Result<()> {
    Cli::parse().run_stdout().await
}
