use std::path::PathBuf;

use dialoguer::{Input, Password};
use owo_colors::OwoColorize;

use crate::{
    commands::init::{DetectedForge, ForkDetection, get_config, set_config},
    config::ForgeType,
    error::Result,
};

/// Initialize GitHub-specific configuration
pub async fn init(
    repo_path: impl Into<PathBuf>,
    detected: Option<&DetectedForge>,
    fork_detection: Option<&ForkDetection>,
) -> Result<()> {
    let repo_path = repo_path.into();
    let existing_host = get_config(&repo_path, "jj-vine.github.host");
    let existing_project = get_config(&repo_path, "jj-vine.github.project");
    let existing_target_project = get_config(&repo_path, "jj-vine.github.targetProject");
    let existing_token = get_config(&repo_path, "jj-vine.github.token");

    let (detected_host, detected_project, detected_target_project) = if let Some(d) = detected {
        if d.forge_type == ForgeType::GitHub {
            let target = fork_detection.and_then(|f| f.target_project.clone());
            (Some(d.host.clone()), Some(d.project.clone()), target)
        } else {
            (None, None, None)
        }
    } else {
        (None, None, None)
    };

    let default_host = existing_host
        .or(detected_host)
        .unwrap_or_else(|| "https://api.github.com".to_string());
    let default_project = existing_project.or(detected_project);

    let github_host = Input::<String>::new()
        .with_prompt(format!(
            "{} {}",
            "GitHub API URL (e.g. https://api.github.com)".bold(),
            "jj-vine.github.host".dimmed()
        ))
        .default(default_host)
        .interact_text()?;

    let github_project = if let Some(project) = default_project {
        Input::<String>::new()
            .with_prompt(format!(
                "{} {}",
                "GitHub repository (owner/repo)".bold(),
                "jj-vine.github.project".dimmed()
            ))
            .default(project)
            .interact_text()?
    } else {
        Input::<String>::new()
            .with_prompt(format!(
                "{} {}",
                "GitHub repository (owner/repo)".bold(),
                "jj-vine.github.project".dimmed()
            ))
            .interact_text()?
    };

    let github_target_project = Input::<String>::new()
        .with_prompt(format!(
            "{} {}",
            "Target repository for PRs (upstream, leave blank for same as source repository)"
                .bold(),
            "jj-vine.github.targetProject".dimmed()
        ))
        .default(
            detected_target_project
                .or(existing_target_project)
                .unwrap_or(github_project.clone()),
        )
        .interact_text()?;

    let github_token = if let Some(token) = existing_token {
        println!(
            "Using existing Personal Access Token. Run `jj config set --repo jj-vine.github.token <token>` to update it."
        );
        token
    } else {
        println!();
        println!("{}", "Personal Access Token required scopes:".yellow());
        println!(
            "  {} {}",
            "•".yellow(),
            "repo (for creating/updating pull requests)".dimmed()
        );
        println!();
        println!(
            "  {}",
            "Create token at: https://github.com/settings/tokens/new".dimmed()
        );
        println!();

        Password::new()
            .with_prompt(format!(
                "{} {}",
                "GitHub Personal Access Token".bold(),
                "jj-vine.github.token".dimmed()
            ))
            .interact()?
    };

    set_config(&repo_path, "jj-vine.github.host", &github_host)?;
    set_config(&repo_path, "jj-vine.github.project", &github_project)?;
    set_config(
        &repo_path,
        "jj-vine.github.targetProject",
        &github_target_project,
    )?;
    set_config(&repo_path, "jj-vine.github.token", &github_token)?;

    Ok(())
}
