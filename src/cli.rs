use std::path::PathBuf;

use clap::{Parser, Subcommand};
use tracing::Level;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

use crate::{
    commands::{status::StatusCommandConfig, submit::SubmitCommandConfig},
    error::Result,
    output::{FlatOutput, InteractiveOutput, Output},
    tracing_formatter::PlainFormatter,
};

#[derive(Parser)]
#[command(name = "jj-vine")]
#[command(about = "GitLab MR submission tool for Jujutsu workflows", long_about = None)]
struct Cli {
    /// Repository path (defaults to current directory)
    #[arg(short = 'R', long, global = true)]
    repository: Option<PathBuf>,

    /// Enable verbose logging
    #[arg(short = 'v', long, global = true)]
    verbose: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Submit a bookmark and its dependencies as GitLab MRs
    Submit(SubmitCommandConfig),

    /// Initialize jj-vine configuration for this repository
    Init,

    /// Show status of tracked bookmarks and their MRs/PRs
    Status(StatusCommandConfig),
}

pub struct CliConfig<'a> {
    /// Repository path (defaults to current directory)
    pub repository: PathBuf,

    /// Output formatter
    pub output: &'a dyn Output,
}

pub async fn cli_main() -> Result<()> {
    let cli = Cli::parse();

    let verbose = cli.verbose || std::env::var("RUST_LOG").is_ok_and(|v| !v.is_empty());
    let filter = EnvFilter::builder()
        .with_default_directive(Level::INFO.into())
        .from_env_lossy();

    if verbose {
        // Verbose mode: Keep timestamps and level
        tracing_subscriber::registry()
            .with(filter)
            .with(
                tracing_subscriber::fmt::layer()
                    .event_format(PlainFormatter::new().with_level(true).with_timestamp(true)),
            )
            .init();
    } else {
        // Default mode: Hide timestamps and level - just show log text
        tracing_subscriber::registry()
            .with(filter)
            .with(tracing_subscriber::fmt::layer().event_format(PlainFormatter::new()))
            .init();
    }

    // Determine repository path
    let repo_path = cli
        .repository
        .unwrap_or_else(|| std::env::current_dir().expect("Failed to get current directory"));

    let can_have_interactive_output = match cli.command {
        Commands::Submit(_) => true,
        Commands::Status(_) => true,
        Commands::Init => false,
    };

    let output: Box<dyn Output> = if verbose || !can_have_interactive_output {
        Box::new(FlatOutput::new())
    } else {
        Box::new(InteractiveOutput::new())
    };

    let main_config = CliConfig {
        repository: repo_path,
        output: output.as_ref(),
    };

    match cli.command {
        Commands::Submit(options) => {
            crate::commands::submit::submit(options, main_config).await?;
            Ok(())
        }
        Commands::Init => {
            crate::commands::init::init(main_config).await?;
            Ok(())
        }
        Commands::Status(options) => {
            crate::commands::status::status(options, main_config).await?;
            Ok(())
        }
    }
}
