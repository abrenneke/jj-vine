use crate::config::Config;
use crate::error::Result;
use crate::gitlab::GitLabClient;
use crate::jj::Jujutsu;
use crate::output;
use crate::submit::{analyze, execute, plan};
use std::path::PathBuf;

/// Submit a bookmark and its dependencies as GitLab MRs
///
/// This orchestrates the three-phase submission process:
/// 1. Analyze - Identify the bookmark stack
/// 2. Plan - Determine what actions are needed
/// 3. Execute - Perform the actions
pub async fn submit(
    repo_path: PathBuf,
    bookmark: String,
    _remote: String,
    dry_run: bool,
) -> Result<()> {
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

    // Phase 1: Analyze
    output::output(&format!("Analyzing bookmark '{}'...", bookmark))?;
    let analysis = analyze::analyze(&jj, &config, &bookmark).await?;

    output::output(&format!(
        "Found {} bookmark(s) to submit: {}",
        analysis.bookmarks_to_submit.len(),
        analysis.bookmarks_to_submit.join(" → ")
    ))?;

    // Phase 2: Plan
    output::output("Creating submission plan...")?;
    let submission_plan = plan::plan(&analysis, &jj, &gitlab, &config, dry_run).await?;

    output::output(&format!(
        "Plan: {} action(s)",
        submission_plan.actions.len()
    ))?;

    // Phase 3: Execute
    output::output("Executing...")?;
    let result = execute::execute(&submission_plan, &jj, &gitlab, &config).await?;

    // Summary
    if dry_run {
        output::output("Dry run complete - no changes made")?;
    } else {
        output::output(&format!(
            "Submission complete: {} MR(s)",
            result.merge_requests.len()
        ))?;

        if !result.errors.is_empty() {
            output::error(&format!("{} error(s) occurred", result.errors.len()))?;
        }

        // Display MR URLs
        for mr in &result.merge_requests {
            output::output(&format!("MR !{}: {}", mr.iid, mr.web_url))?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    // Note: Full integration testing would require setting up a test jj repo and GitLab instance
    // For now, we verify the structure compiles
}
