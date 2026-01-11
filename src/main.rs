use jj_vine::{cli::cli_main, error::Result};

#[tokio::main]
async fn main() -> Result<()> {
    cli_main().await
}
