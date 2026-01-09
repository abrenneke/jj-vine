use crate::bookmark::BookmarkGraph;
use crate::config::Config;
use crate::error::{Error, Result};
use crate::gitlab::GitLabClient;
use crate::jj::Jujutsu;
use crate::output::Output;
use crate::submit::{analyze, execute, plan};
use console::style;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{debug, info};

/// Submit bookmarks and their dependencies as GitLab MRs
///
/// This orchestrates the three-phase submission process for each bookmark:
/// 1. Analyze - Identify the bookmark stack
/// 2. Plan - Determine what actions are needed
/// 3. Execute - Perform the actions
pub async fn submit(
    repo_path: PathBuf,
    bookmarks: Vec<String>,
    _remote: String,
    dry_run: bool,
    verbose: bool,
) -> Result<()> {
    // Create output manager
    let output = Arc::new(Output::new(verbose));

    output.log_message(format!("Submitting bookmarks: {}", bookmarks.join(", ")));

    if bookmarks.is_empty() {
        return Err(Error::Config {
            message: "No bookmarks to submit".to_string(),
        });
    }

    // Load configuration
    debug!("Loading configuration");
    let config = Config::load(&repo_path)?;
    config.validate()?;

    // Create jj and GitLab clients
    debug!("Creating Jujutsu and GitLab clients");
    let jj = Jujutsu::new(repo_path)?;
    let gitlab = GitLabClient::new(
        config.gitlab_host.clone(),
        config.gitlab_project.clone(),
        config.gitlab_token.clone(),
        config.ca_bundle.clone(),
        config.tls_accept_non_compliant_certs,
    )?;

    // Sort bookmarks topologically (dependencies first)
    debug!(
        "Using default branch from config: {}",
        config.default_branch
    );
    let default_branch = &config.default_branch;

    // Build revset for only the bookmarks we're submitting and their ancestors
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

    // Run the three-phase process ONCE for all bookmarks
    debug!("Analyzing {} bookmarks", sorted_bookmarks.len());
    let analysis = analyze::analyze(&jj, &config, &sorted_bookmarks).await?;

    debug!("Creating submission plan");
    let submission_plan =
        plan::plan(&analysis, &jj, &gitlab, &config, &bookmark_graph, dry_run).await?;

    debug!("Executing submission plan");
    let result = execute::execute(&submission_plan, &jj, &gitlab, &config, output.clone()).await?;

    // Finish spinner before showing summary
    output.finish();

    // Display summary
    info!("\n═══════════════════════════════════════");
    info!("{}", style("Summary").bold());
    info!("═══════════════════════════════════════");

    // Show bookmarks pushed
    if !result.bookmarks_pushed.is_empty() {
        info!("Pushed: {}", result.bookmarks_pushed.join(", "));
    }

    // Show all MRs in one unified list
    if !result.mrs_created_details.is_empty()
        || !result.mrs_updated_details.is_empty()
        || !result.mrs_unchanged_details.is_empty()
    {
        info!("");
        info!("{}", style("Merge Requests:").bold());

        // Display created MRs
        for detail in &result.mrs_created_details {
            info!(
                "  {}: {} - {} {}: {}",
                detail.bookmark,
                detail.title,
                style(format!("!{}", detail.iid)).cyan(),
                style("[created]").green(),
                style(&detail.web_url).dim()
            );
        }

        // Display updated MRs
        for detail in &result.mrs_updated_details {
            info!(
                "  {}: {} - {} {}: {}",
                detail.bookmark,
                detail.title,
                style(format!("!{}", detail.iid)).cyan(),
                style("[updated]").green(),
                style(&detail.web_url).dim()
            );
        }

        // Display unchanged MRs (no status label)
        for detail in &result.mrs_unchanged_details {
            info!(
                "  {}: {} - {}: {}",
                detail.bookmark,
                detail.title,
                style(format!("!{}", detail.iid)).cyan(),
                style(&detail.web_url).dim()
            );
        }
    }

    // Show errors
    if !result.errors.is_empty() {
        info!("");
        info!("✗ {} error(s) occurred:", result.errors.len());
        for error in &result.errors {
            info!("  • {}", error);
        }
    }

    // Return error if any errors occurred
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
