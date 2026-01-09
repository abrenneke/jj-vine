use crate::bookmark::BookmarkGraph;
use crate::config::Config;
use crate::error::{Error, Result};
use crate::gitlab::GitLabClient;
use crate::jj::Jujutsu;
use crate::submit::{analyze, execute, plan};
use std::path::PathBuf;
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
) -> Result<()> {
    info!("Starting submit for {} bookmarks", bookmarks.len());

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

    info!(
        "Submission order (topological): {}",
        sorted_bookmarks.join(" → ")
    );

    // Run the three-phase process ONCE for all bookmarks
    debug!("Analyzing {} bookmarks", sorted_bookmarks.len());
    info!("Analyzing {} bookmarks...", sorted_bookmarks.len());
    let analysis = analyze::analyze(&jj, &config, &sorted_bookmarks).await?;

    debug!("Creating submission plan");
    info!("Creating submission plan...");
    let submission_plan =
        plan::plan(&analysis, &jj, &gitlab, &config, &bookmark_graph, dry_run).await?;

    debug!("Executing submission plan");
    info!("Executing plan...");
    let result = execute::execute(&submission_plan, &jj, &gitlab, &config).await?;

    // Display summary
    info!("\n═══════════════════════════════════════");
    info!("Summary");
    info!("═══════════════════════════════════════");

    if result.errors.is_empty() {
        info!(
            "✓ {} bookmark{} submitted",
            analysis.bookmarks_to_submit.len(),
            if analysis.bookmarks_to_submit.len() == 1 {
                ""
            } else {
                "s"
            }
        );
    } else {
        info!("✗ {} error(s) occurred", result.errors.len());
        for error in &result.errors {
            info!("  • {}", error);
        }
        info!("");
    }

    // Show MR status breakdown
    if result.mrs_created > 0 {
        info!("✓ {} MR{} created", result.mrs_created, if result.mrs_created == 1 { "" } else { "s" });
    }
    if result.mrs_updated > 0 {
        info!("✓ {} MR{} updated", result.mrs_updated, if result.mrs_updated == 1 { "" } else { "s" });
    }
    if result.mrs_unchanged > 0 {
        info!("✓ {} MR{} unchanged", result.mrs_unchanged, if result.mrs_unchanged == 1 { "" } else { "s" });
    }

    // Always show links (deduplicated by IID)
    if !result.merge_requests.is_empty() {
        info!("");
        info!("Links:");
        let mut seen_iids = std::collections::HashSet::new();
        for mr in &result.merge_requests {
            if seen_iids.insert(mr.iid) {
                info!("  !{}: {}", mr.iid, mr.web_url);
            }
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

#[cfg(test)]
mod tests {
    // Note: Full integration testing would require setting up a test jj repo and GitLab instance
    // For now, we verify the structure compiles
}
