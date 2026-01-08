use crate::bookmark::BookmarkGraph;
use crate::config::Config;
use crate::error::{Error, Result};
use crate::gitlab::GitLabClient;
use crate::jj::Jujutsu;
use crate::output;
use crate::submit::execute::SubmissionResult;
use crate::submit::{analyze, execute, plan};
use std::path::PathBuf;

/// Submit bookmarks and their dependencies as GitLab MRs
///
/// This orchestrates the three-phase submission process for each bookmark:
/// 1. Analyze - Identify the bookmark stack
/// 2. Plan - Determine what actions are needed
/// 3. Execute - Perform the actions
pub async fn submit(
    repo_path: PathBuf,
    bookmarks: Vec<String>,
    remote: String,
    dry_run: bool,
) -> Result<()> {
    if bookmarks.is_empty() {
        return Err(Error::Config {
            message: "No bookmarks to submit".to_string(),
        });
    }

    // Load configuration
    let config = Config::load(&repo_path)?;
    config.validate()?;

    // Create jj and GitLab clients
    let jj = Jujutsu::new(repo_path)?;
    let gitlab = GitLabClient::new(
        config.gitlab_host.clone(),
        config.gitlab_project.clone(),
        config.gitlab_token.clone(),
        config.ca_bundle.clone(),
        config.tls_accept_non_compliant_certs,
    )?;

    // Sort bookmarks topologically (dependencies first)
    let default_branch = jj.get_default_branch()?;
    let bookmark_graph = BookmarkGraph::build(&jj, &default_branch).await?;
    bookmark_graph.validate_bookmarks(&jj, &bookmarks)?;
    let sorted_bookmarks = bookmark_graph.topological_sort(&bookmarks)?;

    output::output(&format!(
        "Submission order (topological): {}",
        sorted_bookmarks.join(" → ")
    ))?;

    // Submit each bookmark and collect results
    let total = sorted_bookmarks.len();
    let mut all_merge_requests = Vec::new();
    let mut all_errors = Vec::new();
    let mut successful_bookmarks = Vec::new();
    let mut failed_bookmarks = Vec::new();

    for (index, bookmark) in sorted_bookmarks.iter().enumerate() {
        output::output(&format!(
            "\n[{}/{}] Submitting {}...",
            index + 1,
            total,
            bookmark
        ))?;

        match submit_bookmark_stack(bookmark, &jj, &gitlab, &config, &remote, dry_run).await {
            Ok(result) => {
                all_merge_requests.extend(result.merge_requests);
                all_errors.extend(result.errors);
                successful_bookmarks.push(bookmark.clone());
            }
            Err(e) => {
                let error_msg = format!("Failed to submit {}: {}", bookmark, e);
                output::error(&error_msg)?;
                all_errors.push(error_msg.clone());
                failed_bookmarks.push((bookmark.clone(), error_msg));
            }
        }
    }

    // Display unified summary
    output::output("\n═══════════════════════════════════════")?;
    output::output("Summary")?;
    output::output("═══════════════════════════════════════")?;

    if !successful_bookmarks.is_empty() {
        output::output(&format!(
            "✓ Successfully submitted: {} bookmark{}",
            successful_bookmarks.len(),
            if successful_bookmarks.len() == 1 {
                ""
            } else {
                "s"
            }
        ))?;
        for bookmark in &successful_bookmarks {
            output::output(&format!("  • {}", bookmark))?;
        }
        output::output("")?;
    }

    if !failed_bookmarks.is_empty() {
        output::output(&format!(
            "✗ Failed: {} bookmark{}",
            failed_bookmarks.len(),
            if failed_bookmarks.len() == 1 { "" } else { "s" }
        ))?;
        for (bookmark, error) in &failed_bookmarks {
            output::output(&format!("  • {}: {}", bookmark, error))?;
        }
        output::output("")?;
    }

    output::output(&format!(
        "Total: {} MR{} created",
        all_merge_requests.len(),
        if all_merge_requests.len() == 1 {
            ""
        } else {
            "s"
        }
    ))?;

    // Return error if any bookmarks failed
    if !failed_bookmarks.is_empty() {
        return Err(Error::Config {
            message: format!(
                "{} bookmark{} failed to submit",
                failed_bookmarks.len(),
                if failed_bookmarks.len() == 1 { "" } else { "s" }
            ),
        });
    }

    Ok(())
}

/// Submit a single bookmark stack (bookmark and its downstack) as GitLab MRs
///
/// This is the per-bookmark submission logic that handles:
/// 1. Analyze - Identify the bookmark stack
/// 2. Plan - Determine what actions are needed
/// 3. Execute - Perform the actions
async fn submit_bookmark_stack(
    bookmark: &str,
    jj: &Jujutsu,
    gitlab: &GitLabClient,
    config: &Config,
    _remote: &str,
    dry_run: bool,
) -> Result<SubmissionResult> {
    // Phase 1: Analyze
    output::output(&format!("  Analyzing bookmark '{}'...", bookmark))?;
    let analysis = analyze::analyze(jj, config, bookmark).await?;

    output::output(&format!(
        "  Found {} bookmark(s) to submit: {}",
        analysis.bookmarks_to_submit.len(),
        analysis.bookmarks_to_submit.join(" → ")
    ))?;

    // Phase 2: Plan
    output::output("  Creating submission plan...")?;
    let submission_plan = plan::plan(&analysis, jj, gitlab, config, dry_run).await?;

    output::output(&format!(
        "  Plan: {} action(s)",
        submission_plan.actions.len()
    ))?;

    // Phase 3: Execute
    output::output("  Executing...")?;
    let result = execute::execute(&submission_plan, jj, gitlab, config).await?;

    // Per-bookmark summary
    if dry_run {
        output::output("  Dry run complete - no changes made")?;
    } else {
        output::output(&format!(
            "  Submission complete: {} MR(s)",
            result.merge_requests.len()
        ))?;

        if !result.errors.is_empty() {
            output::error(&format!("  {} error(s) occurred", result.errors.len()))?;
        }

        // Display MR URLs
        for mr in &result.merge_requests {
            output::output(&format!("  MR !{}: {}", mr.iid, mr.web_url))?;
        }
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    // Note: Full integration testing would require setting up a test jj repo and GitLab instance
    // For now, we verify the structure compiles
}
