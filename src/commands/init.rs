use std::path::PathBuf;

use dialoguer::{Input, Password};
use owo_colors::OwoColorize;

use crate::{cli::CliConfig, error::Result, jj::run_jj_command};

/// Initialize jj-vine configuration for this repository
pub async fn init(cli_config: CliConfig<'_>) -> Result<()> {
    println!("This will configure jj-vine for your GitLab instance.");
    println!(
        "{}",
        "Configuration will be stored in .jj/repo/config.toml".dimmed()
    );
    println!();

    let (detected_host, detected_project) = detect_from_remote(&cli_config.repository)?;

    let gitlab_host = if let Some(host) = &detected_host {
        Input::<String>::new()
            .with_prompt(format!("{}", "GitLab instance URL".bold()))
            .default(host.clone())
            .interact_text()?
    } else {
        Input::<String>::new()
            .with_prompt(format!(
                "{}",
                "GitLab instance URL (e.g. https://gitlab.example.com)".bold()
            ))
            .interact_text()?
    };

    let gitlab_project = if let Some(project) = &detected_project {
        Input::<String>::new()
            .with_prompt(format!("{}", "GitLab project ID".bold()))
            .default(project.clone())
            .interact_text()?
    } else {
        println!("{}", "Project ID can be either:".dimmed());
        println!(
            "{}",
            "  - Group/project path (e.g., my-group/my-project)".dimmed()
        );
        println!("{}", "  - Numeric project ID (e.g., 12345)".dimmed());

        Input::<String>::new()
            .with_prompt(format!("{}", "GitLab project ID".bold()))
            .interact_text()?
    };

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
        format!(
            "Create token at: {}/-/user_settings/personal_access_tokens",
            gitlab_host
        )
        .dimmed()
    );
    println!();

    let gitlab_token = Password::new()
        .with_prompt(format!("{}", "GitLab Personal Access Token".bold()))
        .interact()?;

    let remote_name = Input::<String>::new()
        .with_prompt(format!("{}", "Remote name".bold()))
        .default("origin".to_string())
        .interact_text()?;

    let default_branch = Input::<String>::new()
        .with_prompt(format!("{}", "Default branch".bold()))
        .default("main".to_string())
        .interact_text()?;

    set_config(&cli_config.repository, "jj-vine.gitlabHost", &gitlab_host)?;
    set_config(
        &cli_config.repository,
        "jj-vine.gitlabProject",
        &gitlab_project,
    )?;
    set_config(&cli_config.repository, "jj-vine.gitlabToken", &gitlab_token)?;
    set_config(&cli_config.repository, "jj-vine.remoteName", &remote_name)?;
    set_config(
        &cli_config.repository,
        "jj-vine.defaultBranch",
        &default_branch,
    )?;

    println!();
    println!(
        "{} {}",
        "✓".green().bold(),
        "Configuration complete!".green()
    );
    println!("{}", "You can now use: jj mr submit <bookmark>".cyan());

    Ok(())
}

/// Set a configuration value using jj config set
fn set_config(repo_path: &PathBuf, key: &str, value: &str) -> Result<()> {
    run_jj_command(repo_path, &["config", "set", "--repo", key, value])?;
    Ok(())
}

/// Detect GitLab host and project from git remote
fn detect_from_remote(repo_path: &PathBuf) -> Result<(Option<String>, Option<String>)> {
    // Get the origin remote URL
    let remote_output = match run_jj_command(repo_path, &["git", "remote", "list"]) {
        Ok(output) => output,
        Err(_) => return Ok((None, None)),
    };

    // Parse the first remote (typically origin)
    for line in remote_output.stdout.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            let url = parts[1];

            // Try to parse GitLab URL
            // Supports: git@host:group/project.git, https://host/group/project.git
            if let Some((host, project)) = parse_gitlab_url(url) {
                return Ok((Some(host), Some(project)));
            }
        }
    }

    Ok((None, None))
}

/// Parse a GitLab remote URL to extract host and project
fn parse_gitlab_url(url: &str) -> Option<(String, String)> {
    // SSH format: git@gitlab.example.com:group/project.git
    if url.starts_with("git@") {
        let rest = url.strip_prefix("git@")?;
        let (host, path) = rest.split_once(':')?;
        let project = path.strip_suffix(".git").unwrap_or(path);
        return Some((format!("https://{}", host), project.to_string()));
    }

    // HTTPS format: https://gitlab.example.com/group/project.git
    if url.starts_with("https://") || url.starts_with("http://") {
        let without_protocol = url
            .strip_prefix("https://")
            .or_else(|| url.strip_prefix("http://"))?;

        let (host, path) = without_protocol.split_once('/')?;
        let project = path.strip_suffix(".git").unwrap_or(path);

        let protocol = if url.starts_with("https://") {
            "https"
        } else {
            "http"
        };

        return Some((format!("{}://{}", protocol, host), project.to_string()));
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_gitlab_url_ssh() {
        let url = "git@gitlab.example.com:group/project.git";
        let result = parse_gitlab_url(url);
        assert_eq!(
            result,
            Some((
                "https://gitlab.example.com".to_string(),
                "group/project".to_string()
            ))
        );
    }

    #[test]
    fn test_parse_gitlab_url_https() {
        let url = "https://gitlab.example.com/group/project.git";
        let result = parse_gitlab_url(url);
        assert_eq!(
            result,
            Some((
                "https://gitlab.example.com".to_string(),
                "group/project".to_string()
            ))
        );
    }

    #[test]
    fn test_parse_gitlab_url_no_git_suffix() {
        let url = "git@gitlab.example.com:group/project";
        let result = parse_gitlab_url(url);
        assert_eq!(
            result,
            Some((
                "https://gitlab.example.com".to_string(),
                "group/project".to_string()
            ))
        );
    }

    #[test]
    fn test_parse_gitlab_url_nested_groups() {
        let url = "git@gitlab.example.com:group/subgroup/project.git";
        let result = parse_gitlab_url(url);
        assert_eq!(
            result,
            Some((
                "https://gitlab.example.com".to_string(),
                "group/subgroup/project".to_string()
            ))
        );
    }
}
