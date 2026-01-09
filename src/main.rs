use clap::{Parser, Subcommand};
use jj_mrs::commands::{init, submit};
use jj_mrs::error::{Error, Result};
use jj_mrs::jj::Jujutsu;
use jj_mrs::tracing_formatter::PlainFormatter;
use std::env;
use std::path::PathBuf;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

#[derive(Parser)]
#[command(name = "jj-mrs")]
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
    Submit {
        /// The bookmark to submit (mutually exclusive with --tracked)
        bookmark: Option<String>,

        /// Submit all tracked bookmarks (mutually exclusive with bookmark)
        #[arg(long)]
        tracked: bool,

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

    // Initialize tracing
    let filter = if cli.verbose {
        EnvFilter::new("debug")
    } else {
        EnvFilter::new("info")
    };

    if cli.verbose {
        // Verbose mode: Keep timestamps and level
        tracing_subscriber::registry()
            .with(filter)
            .with(fmt::layer().event_format(PlainFormatter::new().with_level(true).with_timestamp(true)))
            .init();
    } else {
        // Default mode: Hide timestamps and level - just show log text
        tracing_subscriber::registry()
            .with(filter)
            .with(fmt::layer().event_format(PlainFormatter::new()))
            .init();
    }

    // Determine repository path
    let repo_path = cli
        .repository
        .unwrap_or_else(|| env::current_dir().expect("Failed to get current directory"));

    match cli.command {
        Commands::Submit {
            bookmark,
            tracked,
            remote,
            dry_run,
        } => {
            // Validate: either bookmark or tracked must be set, but not both
            let bookmarks = match (bookmark, tracked) {
                (Some(_), true) => {
                    return Err(Error::Config {
                        message: "Cannot specify both a bookmark and --tracked flag. Please use one or the other.".to_string(),
                    });
                }
                (None, false) => {
                    return Err(Error::Config {
                        message: "Must specify either a bookmark or use --tracked flag".to_string(),
                    });
                }
                (Some(bookmark), false) => {
                    // Single bookmark mode
                    vec![bookmark]
                }
                (None, true) => {
                    // Tracked bookmarks mode
                    let jj = Jujutsu::new(repo_path.clone())?;
                    let tracked_bookmarks = jj.get_tracked_bookmarks(&remote)?;

                    if tracked_bookmarks.is_empty() {
                        return Err(Error::Config {
                            message: "No tracked bookmarks found. Tracked bookmarks must be authored by you and pushed to remote.".to_string(),
                        });
                    }

                    tracked_bookmarks
                }
            };

            submit::submit(repo_path, bookmarks, remote, dry_run, cli.verbose).await?;
            Ok(())
        }
        Commands::Init => {
            init::init(repo_path).await?;
            Ok(())
        }
    }
}
