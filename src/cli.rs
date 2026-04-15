use std::path::PathBuf;

use clap::{Parser, Subcommand};
use tracing::Level;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

use crate::{
    commands::{status::StatusCommandConfig, submit::SubmitCommandConfig},
    error::Result,
    output::{BufferedOutput, FlatOutput, InteractiveOutput, Output},
    tracing_formatter::PlainFormatter,
};

#[derive(Parser)]
#[command(name = "jj-vine")]
#[command(about = "GitLab MR submission tool for Jujutsu workflows", long_about = None)]
pub struct Cli {
    /// Repository path (defaults to current directory)
    #[arg(short = 'R', long, global = true)]
    pub repository: Option<PathBuf>,

    /// Enable verbose logging
    #[arg(short = 'v', long, global = true, default_value_t = Cli::default_verbosity())]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    #[command(about = SubmitCommandConfig::help_long())]
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

impl Cli {
    pub fn default_verbosity() -> bool {
        std::env::var("RUST_LOG").is_ok_and(|v| !v.is_empty())
    }

    pub async fn run_stdout(&self) -> Result<()> {
        let can_have_interactive_output = match self.command {
            Commands::Submit(_) => true,
            Commands::Status(_) => true,
            Commands::Init => false,
        };

        let output: Box<dyn Output> = if self.verbose || !can_have_interactive_output {
            Box::new(FlatOutput::new())
        } else {
            Box::new(InteractiveOutput::new())
        };

        let filter = EnvFilter::builder()
            .with_default_directive(Level::INFO.into())
            .from_env_lossy();

        if self.verbose {
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

        self.run(output.as_ref()).await
    }

    pub async fn run_captured(&self) -> Result<String> {
        let buffered_output = BufferedOutput::new();
        self.run(&buffered_output).await?;
        Ok(strip_ansi::strip_str(&buffered_output.get_buffer()).to_string())
    }

    pub async fn run(&self, output: &dyn Output) -> Result<()> {
        let repo_path =
            self.repository.as_ref().map(Into::into).unwrap_or_else(|| {
                std::env::current_dir().expect("Failed to get current directory")
            });

        let main_config = CliConfig {
            repository: repo_path.to_path_buf(),
            output,
        };

        match &self.command {
            Commands::Submit(options) => {
                crate::commands::submit::submit(options, &main_config).await?;
                Ok(())
            }
            Commands::Init => {
                crate::commands::init::init(&main_config).await?;
                Ok(())
            }
            Commands::Status(options) => {
                crate::commands::status::status(options, &main_config).await?;
                Ok(())
            }
        }
    }
}
