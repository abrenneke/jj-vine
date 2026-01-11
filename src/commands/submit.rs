use std::path::PathBuf;

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
    config::Config,
    error::{Error, Result},
    gitlab::GitLabClient,
    jj::Jujutsu,
    output::{FlatOutput, InteractiveOutput, Output},
    submit::{
        analyze,
        execute,
        execute::{MRUpdate, MRUpdateType},
        plan,
    },
};

/// Submit bookmarks and their dependencies as GitLab MRs
pub async fn submit(
    repo_path: PathBuf,
    bookmarks: Vec<String>,
    _remote: String,
    dry_run: bool,
    verbose: bool,
) -> Result<()> {
    let output: Box<dyn Output> = if verbose {
        Box::new(FlatOutput::new())
    } else {
        Box::new(InteractiveOutput::new())
    };

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
    let config = Config::load(&repo_path)?;

    debug!("Creating Jujutsu and GitLab clients");
    let jj = Jujutsu::new(repo_path)?;
    let gitlab = GitLabClient::new(
        config.gitlab_host.clone(),
        config.gitlab_project.clone(),
        config.gitlab_token.clone(),
        config.ca_bundle.clone(),
        config.tls_accept_non_compliant_certs,
    )?;

    debug!(
        "Using default branch from config: {}",
        config.default_branch
    );
    let default_branch = &config.default_branch;

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
    let analysis = analyze::analyze(&jj, &config, &sorted_bookmarks).await?;

    debug!("Creating submission plan");
    output.log_current("Planning submission");
    let submission_plan = plan::plan(
        &analysis,
        &jj,
        &gitlab,
        &config,
        &bookmark_graph,
        dry_run,
        &*output,
    )
    .await?;

    debug!("Executing submission plan");
    let result = execute::execute(&submission_plan, &jj, &gitlab, &config, &*output).await?;

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
