use std::path::PathBuf;

use dialoguer::{Input, Password};
use owo_colors::OwoColorize as _;

use crate::{
    commands::init::{Remotes, get_config, parse_forge_url, set_config},
    error::Result,
};

/// Initialize GitLab-specific configuration.
#[expect(clippy::single_call_fn, reason = "important")]
#[expect(clippy::too_many_lines, reason = "important")]
pub fn init(repo_path: impl Into<PathBuf>, remotes: Option<&Remotes>) -> Result<()> {
    let repo_path = repo_path.into();
    let existing_host = get_config(&repo_path, "jj-vine.gitlab.host");
    let existing_project = get_config(&repo_path, "jj-vine.gitlab.project");
    let existing_target_project = get_config(&repo_path, "jj-vine.gitlab.targetProject");
    let existing_token = get_config(&repo_path, "jj-vine.gitlab.token");

    let remotes = remotes.as_ref();
    let source_forge = remotes.and_then(|r| parse_forge_url(&r.origin));
    let target_forge = remotes.and_then(|r| {
        r.upstream
            .as_deref()
            .and_then(parse_forge_url)
            .or_else(|| parse_forge_url(&r.origin))
    });

    let default_host = existing_host.or(target_forge.as_ref().map(|f| f.host.clone()));
    let default_project = existing_project.or(source_forge
        .as_ref()
        .or(target_forge.as_ref())
        .map(|f| f.project.clone()));

    let gitlab_host = if let Some(host) = default_host {
        Input::<String>::new()
            .with_prompt(format!(
                "{} {}",
                "GitLab instance URL (e.g. https://gitlab.example.com)".bold(),
                "jj-vine.gitlab.host".dimmed()
            ))
            .default(host)
            .interact_text()?
    } else {
        Input::<String>::new()
            .with_prompt(format!(
                "{} {}",
                "GitLab instance URL (e.g. https://gitlab.example.com)".bold(),
                "jj-vine.gitlab.host".dimmed()
            ))
            .interact_text()?
    };

    let gitlab_project = if let Some(project) = default_project {
        Input::<String>::new()
            .with_prompt(format!(
                "{} {}",
                "GitLab project ID (e.g. group/project)".bold(),
                "jj-vine.gitlab.project".dimmed()
            ))
            .default(project)
            .interact_text()?
    } else {
        Input::<String>::new()
            .with_prompt(format!(
                "{} {}",
                "GitLab project ID (e.g. group/project)".bold(),
                "jj-vine.gitlab.project".dimmed()
            ))
            .interact_text()?
    };

    let gitlab_target_project = Input::<String>::new()
        .with_prompt(format!(
            "{} {}",
            "Target project for MRs (upstream, leave blank for same as source project)".bold(),
            "jj-vine.gitlab.targetProject".dimmed()
        ))
        .default(
            existing_target_project
                .or(target_forge.as_ref().map(|f| f.project.clone()))
                .unwrap_or(gitlab_project.clone()),
        )
        .interact_text()?;

    let gitlab_token = if let Some(token) = existing_token {
        println!(
            "Using existing Personal Access Token. Run `jj config set --repo jj-vine.gitlab.token <token>` to update it."
        );
        token
    } else {
        println!();
        println!("{}", "Personal Access Token required scopes:".yellow());
        println!(
            "  {} {}",
            "•".yellow(),
            "api (for creating/updating merge requests)".dimmed()
        );
        println!();
        println!(
            "{} {}",
            "⚠".yellow(),
            "Note: GitLab does not offer more granular scopes for MR operations.".dimmed()
        );
        println!(
            "  {}",
            "The 'api' scope grants full read/write API access.".dimmed()
        );
        println!(
            "  {}",
            format!("Create token at: {gitlab_host}/-/user_settings/personal_access_tokens")
                .dimmed()
        );
        println!();

        Password::new()
            .with_prompt(format!(
                "{} {}",
                "GitLab Personal Access Token".bold(),
                "jj-vine.gitlab.token".dimmed()
            ))
            .interact()?
    };

    set_config(&repo_path, "jj-vine.gitlab.host", &gitlab_host)?;
    set_config(&repo_path, "jj-vine.gitlab.project", &gitlab_project)?;
    set_config(
        &repo_path,
        "jj-vine.gitlab.targetProject",
        &gitlab_target_project,
    )?;
    set_config(&repo_path, "jj-vine.gitlab.token", &gitlab_token)?;

    Ok(())
}
