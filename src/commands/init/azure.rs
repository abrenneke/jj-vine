use std::path::PathBuf;

use dialoguer::{Input, Password};
use owo_colors::OwoColorize;

use crate::{
    commands::init::{Remotes, get_config, set_config},
    error::Result,
};

#[allow(clippy::too_many_lines, reason = "important")]
pub fn init(repo_path: impl Into<PathBuf>, remotes: Option<&Remotes>) -> Result<()> {
    let repo_path = repo_path.into();
    let existing_host = get_config(&repo_path, "jj-vine.azure.host");
    let existing_vssps_host = get_config(&repo_path, "jj-vine.azure.vsspsHost");
    let existing_project = get_config(&repo_path, "jj-vine.azure.project");
    let existing_target_project = get_config(&repo_path, "jj-vine.azure.targetProject");
    let existing_token = get_config(&repo_path, "jj-vine.azure.token");
    let existing_source_repository_name =
        get_config(&repo_path, "jj-vine.azure.sourceRepositoryName");
    let existing_target_repository_name =
        get_config(&repo_path, "jj-vine.azure.targetRepositoryName");

    let remotes = remotes.as_ref();
    let forge = match remotes {
        Some(Remotes {
            target_forge: Some(forge),
            ..
        }) => Some(forge),
        _ => None,
    };

    let mut default_host = existing_host
        .or(forge.map(|f| f.host.clone()))
        .unwrap_or_else(|| "https://dev.azure.com".to_string());

    // Handle ssh.dev.azure.com
    if default_host.starts_with("https://ssh.") {
        default_host = default_host.replace("https://ssh.", "https://");
    }

    let azure_host = Input::<String>::new()
        .with_prompt(format!(
            "{} {}",
            "Azure DevOps API URL (e.g. https://dev.azure.com)".bold(),
            "jj-vine.azure.host".dimmed()
        ))
        .default(default_host)
        .interact_text()?;

    let default_vssps_host = existing_vssps_host.unwrap_or_else(|| {
        format!(
            "https://vssps.{}",
            azure_host.trim_start_matches("https://")
        )
    });

    let azure_vssps_host = Input::<String>::new()
        .with_prompt(format!(
            "{} {}",
            "Azure DevOps Security (VSSP) API URL (e.g. https://vssps.dev.azure.com). Used to look up other users for automatic review requests.".bold(),
            "jj-vine.azure.vsspsHost".dimmed()
        ))
        .allow_empty(true)
        .default(default_vssps_host)
        .interact_text()?;

    let default_project = existing_project.or(forge.map(|f| f.project.clone()));

    let azure_project = if let Some(project) = default_project {
        Input::<String>::new()
            .with_prompt(format!(
                "{} {}",
                "Azure DevOps project (organization/project)".bold(),
                "jj-vine.azure.project".dimmed()
            ))
            .default(project)
            .interact_text()?
    } else {
        Input::<String>::new()
            .with_prompt(format!(
                "{} {}",
                "Azure DevOps project (organization/project)".bold(),
                "jj-vine.azure.project".dimmed()
            ))
            .interact_text()?
    };

    let default_target_project = existing_target_project
        .or(remotes.and_then(|f| f.upstream.clone()))
        .or(remotes.map(|r| r.origin.clone()));

    let azure_target_project = Input::<String>::new()
        .with_prompt(format!(
            "{} {}",
            "Target project for PRs (upstream, leave blank for same as source project)".bold(),
            "jj-vine.azure.targetProject".dimmed()
        ))
        .default(default_target_project.unwrap_or_default())
        .interact_text()?;

    let default_source_repository_name =
        existing_source_repository_name.or(forge.and_then(|f| f.repository_name.clone()));

    let azure_source_repository_name = if let Some(repository_name) = default_source_repository_name
    {
        Input::<String>::new()
            .with_prompt(format!(
                "{} {}",
                "Name of the repository where branches are pushed (source/fork project)".bold(),
                "jj-vine.azure.sourceRepositoryName".dimmed()
            ))
            .default(repository_name)
            .interact_text()?
    } else {
        Input::<String>::new()
            .with_prompt(format!(
                "{} {}",
                "Name of the repository where branches are pushed (source/fork project)".bold(),
                "jj-vine.azure.sourceRepositoryName".dimmed()
            ))
            .interact_text()?
    };

    let default_target_repository_name =
        existing_target_repository_name.unwrap_or(azure_source_repository_name.clone());

    let azure_target_repository_name = Input::<String>::new()
        .with_prompt(format!(
            "{} {}",
            "Name of the repository where MRs/PRs are created (target/upstream project)".bold(),
            "jj-vine.azure.targetRepositoryName".dimmed()
        ))
        .default(default_target_repository_name)
        .interact_text()?;

    let azure_token = if let Some(token) = existing_token {
        println!(
            "Using existing Personal Access Token. Run `jj config set --repo jj-vine.azure.token <token>` to update it."
        );
        token
    } else {
        let (org, _project) = azure_project.split_once('/').unwrap();

        println!();
        println!(
            "  {}",
            format!(
                "Create token at: {}/{}/_usersSettings/tokens",
                azure_host.trim_end_matches('/'),
                org
            )
            .dimmed()
        );
        println!();

        Password::new()
            .with_prompt(format!(
                "{} {}",
                "Azure DevOps Personal Access Token".bold(),
                "jj-vine.azure.token".dimmed()
            ))
            .interact()?
    };

    set_config(&repo_path, "jj-vine.azure.host", &azure_host)?;
    set_config(&repo_path, "jj-vine.azure.vsspsHost", &azure_vssps_host)?;
    set_config(&repo_path, "jj-vine.azure.project", &azure_project)?;
    set_config(
        &repo_path,
        "jj-vine.azure.targetProject",
        &azure_target_project,
    )?;
    set_config(
        &repo_path,
        "jj-vine.azure.sourceRepositoryName",
        &azure_source_repository_name,
    )?;
    set_config(
        &repo_path,
        "jj-vine.azure.targetRepositoryName",
        &azure_target_repository_name,
    )?;
    set_config(&repo_path, "jj-vine.azure.token", &azure_token)?;

    Ok(())
}
