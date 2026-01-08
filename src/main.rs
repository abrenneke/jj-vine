use clap::{Parser, Subcommand};
use jj_mrs::commands::{init, submit};
use jj_mrs::error::Result;
use std::env;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "jj-mrs")]
#[command(about = "GitLab MR submission tool for Jujutsu workflows", long_about = None)]
struct Cli {
    /// Repository path (defaults to current directory)
    #[arg(short = 'R', long, global = true)]
    repository: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Submit a bookmark and its dependencies as GitLab MRs
    Submit {
        /// The bookmark to submit
        bookmark: String,

        /// Remote to push to
        #[arg(long, default_value = "origin")]
        remote: String,

        /// Dry run - don't actually push or create MRs
        #[arg(long)]
        dry_run: bool,
    },

    /// Initialize jj-mrs configuration for this repository
    Init,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Determine repository path
    let repo_path = cli
        .repository
        .unwrap_or_else(|| env::current_dir().expect("Failed to get current directory"));

    match cli.command {
        Commands::Submit {
            bookmark,
            remote,
            dry_run,
        } => {
            submit::submit(repo_path, bookmark, remote, dry_run).await?;
            Ok(())
        }
        Commands::Init => {
            init::init(repo_path).await?;
            Ok(())
        }
    }
}
