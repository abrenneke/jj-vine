use clap::Args;
use cli_table::{
    Cell,
    Table,
    format::{Border, Separator},
};
use itertools::Itertools;
use owo_colors::OwoColorize;
use tracing::{debug, info};

use crate::{
    bookmark::BookmarkGraph,
    cli::CliConfig,
    config::Config,
    error::{Error, Result},
    gitlab::GitLabClient,
    jj::Jujutsu,
    submit::{
        analyze,
        execute::{self, MRUpdate, MRUpdateType},
        plan,
    },
};

#[derive(Args)]
pub struct SubmitCommandConfig {
    /// The bookmark to submit (mutually exclusive with --tracked)
    pub bookmark: Option<String>,

    /// Submit all tracked bookmarks (mutually exclusive with bookmark)
    #[arg(long)]
    pub tracked: bool,

    /// Remote to push to
    #[arg(long, default_value = "origin")]
    pub remote: String,

    /// Dry run - don't actually push or create MRs
    #[arg(long)]
    pub dry_run: bool,
}

impl Default for SubmitCommandConfig {
    fn default() -> Self {
        Self {
            bookmark: None,
            tracked: false,
            remote: "origin".to_string(),
            dry_run: false,
        }
    }
}

/// Submit bookmarks and their dependencies as GitLab MRs
pub async fn submit(config: SubmitCommandConfig, cli_config: CliConfig<'_>) -> Result<()> {
    debug!("Creating Jujutsu and GitLab clients");
    let jj = Jujutsu::new(cli_config.repository.clone())?;

    let bookmarks = get_bookmarks(&config, &jj)?;
    let output = cli_config.output;

    output.log_message(&format!(
        "Submitting bookmarks: {}",
        bookmarks
            .iter()
            .map(|b| b.magenta().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    ));

    if bookmarks.is_empty() {
        return Err(Error::Config {
            message: "No bookmarks to submit".to_string(),
        });
    }

    debug!("Loading configuration");
    let repo_config = Config::load(&cli_config.repository)?;

    let gitlab = GitLabClient::new(
        repo_config.gitlab_host.clone(),
        repo_config.gitlab_project.clone(),
        repo_config.gitlab_token.clone(),
        repo_config.ca_bundle.clone(),
        repo_config.tls_accept_non_compliant_certs,
    )?;

    debug!(
        "Using default branch from config: {}",
        repo_config.default_branch
    );
    let default_branch = &repo_config.default_branch;

    let revset = format!(
        "({}) & mine() & bookmarks()",
        bookmarks
            .iter()
            .map(|b| format!("::{}", b))
            .collect::<Vec<_>>()
            .join(" | ")
    );
    debug!("Querying bookmarks with revset: {}", revset);
    let relevant_bookmarks = jj.get_bookmarks_with_revset(&revset)?;
    debug!(
        "Got {} relevant bookmarks for submission",
        relevant_bookmarks.len()
    );

    debug!(
        "Building bookmark graph for default branch: {}",
        default_branch
    );
    let bookmark_graph = BookmarkGraph::build(&jj, default_branch, relevant_bookmarks).await?;
    debug!("Validating bookmarks");
    bookmark_graph.validate_bookmarks(&jj, &bookmarks)?;
    debug!("Performing topological sort");
    let sorted_bookmarks = bookmark_graph.topological_sort(&bookmarks)?;

    debug!(
        "Submission order (topological): {}",
        sorted_bookmarks.join(" → ")
    );

    debug!("Analyzing {} bookmarks", sorted_bookmarks.len());
    output.log_current("Analyzing bookmarks");
    let analysis = analyze::analyze(&jj, &repo_config, &sorted_bookmarks).await?;

    debug!("Creating submission plan");
    output.log_current("Planning submission");
    let submission_plan = plan::plan(
        &analysis,
        &jj,
        &gitlab,
        &repo_config,
        &bookmark_graph,
        config.dry_run,
        output,
    )
    .await?;

    debug!("Executing submission plan");
    let result = execute::execute(&submission_plan, &jj, &gitlab, &repo_config, output).await?;

    output.finish();

    info!("\n═══════════════════════════════════════");
    info!("{}", "Summary".bold());
    info!("═══════════════════════════════════════");

    if !result.bookmarks_pushed.is_empty() {
        let formatted_bookmarks: Vec<String> = result
            .bookmarks_pushed
            .iter()
            .map(|b| b.magenta().to_string())
            .collect();
        info!("Pushed: {}", formatted_bookmarks.join(", "));
    } else {
        info!("No bookmarks pushed");
    }

    if !result.merge_requests.is_empty() {
        info!("\n{}\n", "Merge Requests:".bold());

        let mut table = vec![];

        for MRUpdate {
            mr,
            bookmark,
            update_type,
        } in result.merge_requests.iter().sorted_by_key(|mr| mr.mr.iid)
        {
            match update_type {
                MRUpdateType::Created => {
                    table.push(vec![
                        bookmark.magenta().cell(),
                        mr.title.clone().cell(),
                        mr.web_url.dimmed().cell(),
                        "[created]".green().cell(),
                    ]);
                }
                MRUpdateType::Repointed { .. }
                | MRUpdateType::Both { .. }
                | MRUpdateType::DescriptionUpdated => {
                    table.push(vec![
                        bookmark.magenta().cell(),
                        mr.title.clone().cell(),
                        mr.web_url.dimmed().cell(),
                        "[updated]".green().cell(),
                    ]);
                }
                MRUpdateType::Unchanged => {
                    table.push(vec![
                        bookmark.magenta().cell(),
                        mr.title.clone().cell(),
                        mr.web_url.dimmed().cell(),
                        " ".cell(),
                    ]);
                }
            }
        }

        info!(
            "{}",
            table
                .table()
                .border(Border::builder().build())
                .separator(Separator::builder().build())
                .display()
                .expect("Failed to display table")
        );
    }

    if !result.errors.is_empty() {
        info!("");
        info!("✗ {} error(s) occurred:", result.errors.len());
        for error in &result.errors {
            info!("  • {}", error);
        }
    }

    if !result.errors.is_empty() {
        return Err(Error::Config {
            message: format!(
                "{} error(s) occurred during submission",
                result.errors.len()
            ),
        });
    }

    Ok(())
}

/// Validate: either bookmark or tracked must be set, but not both
fn get_bookmarks(config: &SubmitCommandConfig, jj: &Jujutsu) -> Result<Vec<String>> {
    match (&config.bookmark, config.tracked) {
        (Some(_), true) => Err(Error::Config {
            message:
                "Cannot specify both a bookmark and --tracked flag. Please use one or the other."
                    .to_string(),
        }),
        (None, false) => Err(Error::Config {
            message: "Must specify either a bookmark or use --tracked flag".to_string(),
        }),
        (Some(bookmark), false) => {
            // Single bookmark mode
            Ok(vec![bookmark.clone()])
        }
        (None, true) => {
            let tracked_bookmarks = jj.get_tracked_bookmarks(&config.remote)?;

            if tracked_bookmarks.is_empty() {
                return Err(Error::Config {
                    message: "No tracked bookmarks found. Tracked bookmarks must be authored by you and pushed to remote.".to_string(),
                });
            }

            Ok(tracked_bookmarks)
        }
    }
}
