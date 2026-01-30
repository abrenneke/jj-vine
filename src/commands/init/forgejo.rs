use std::path::PathBuf;

use dialoguer::{Input, Password};
use owo_colors::OwoColorize;

use crate::{
    commands::init::{Remotes, get_config, set_config},
    error::Result,
};

/// Initialize Forgejo/Codeburg/Gitea-specific configuration
pub async fn init(repo_path: impl Into<PathBuf>, remotes: Option<Remotes>) -> Result<()> {
    let repo_path = repo_path.into();
    let existing_host = get_config(&repo_path, "jj-vine.forgejo.host");
    let existing_project = get_config(&repo_path, "jj-vine.forgejo.project");
    let existing_target_project = get_config(&repo_path, "jj-vine.forgejo.targetProject");
    let existing_token = get_config(&repo_path, "jj-vine.forgejo.token");

    let remotes = remotes.as_ref();
    let forge = match remotes {
        Some(Remotes {
            target_forge: Some(forge),
            ..
        }) => Some(forge),
        _ => None,
    };

    let default_host = existing_host
        .or(forge.map(|f| f.host.clone()))
        .unwrap_or_else(|| "https://codeberg.org".to_string());
    let default_project = existing_project.or(forge.map(|f| f.project.clone()));

    let forgejo_host = Input::<String>::new()
        .with_prompt(format!(
            "{} {}",
            "Forgejo/Codeburg/Gitea instance URL (e.g. https://codeberg.org)".bold(),
            "jj-vine.forgejo.host".dimmed()
        ))
        .default(default_host)
        .interact_text()?;

    let forgejo_project = if let Some(project) = default_project {
        Input::<String>::new()
            .with_prompt(format!(
                "{} {}",
                "Forgejo/Codeburg/Gitea repository (owner/repo)".bold(),
                "jj-vine.forgejo.project".dimmed()
            ))
            .default(project)
            .interact_text()?
    } else {
        Input::<String>::new()
            .with_prompt(format!(
                "{} {}",
                "Forgejo/Codeburg/Gitea repository (owner/repo)".bold(),
                "jj-vine.forgejo.project".dimmed()
            ))
            .interact_text()?
    };

    let forgejo_target_project = Input::<String>::new()
        .with_prompt(format!(
            "{} {}",
            "Target repository for PRs (upstream, leave blank for same as source repository)"
                .bold(),
            "jj-vine.forgejo.targetProject".dimmed()
        ))
        .default(
            existing_target_project
                .or(remotes.and_then(|f| f.upstream.clone()))
                .or(remotes.map(|r| r.origin.clone()))
                .unwrap_or(forgejo_project.clone()),
        )
        .interact_text()?;

    let forgejo_token = if let Some(token) = existing_token {
        println!(
            "Using existing Personal Access Token. Run `jj config set --repo jj-vine.forgejo.token <token>` to update it."
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
            format!(
                "Create token at: {}/user/settings/applications",
                forgejo_host
            )
            .dimmed(),
        );
        println!();

        Password::new()
            .with_prompt(format!(
                "{} {}",
                "Forgejo/Codeburg/Gitea Personal Access Token".bold(),
                "jj-vine.forgejo.token".dimmed()
            ))
            .interact()?
    };

    set_config(&repo_path, "jj-vine.forgejo.host", &forgejo_host)?;
    set_config(&repo_path, "jj-vine.forgejo.project", &forgejo_project)?;
    set_config(
        &repo_path,
        "jj-vine.forgejo.targetProject",
        &forgejo_target_project,
    )?;
    set_config(&repo_path, "jj-vine.forgejo.token", &forgejo_token)?;

    Ok(())
}
