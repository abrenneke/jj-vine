use crate::error::Result;
use crate::jj::run_jj_command;
use console::{Term, style};
use dialoguer::{Input, Password};
use std::path::PathBuf;

/// Initialize jj-mrs configuration for this repository
pub async fn init(repo_path: PathBuf) -> Result<()> {
    let term = Term::stdout();

    term.write_line(&format!(
        "{}",
        style("This will configure jj-mrs for your GitLab instance.")
    ))?;
    term.write_line(&format!(
        "{}",
        style("Configuration will be stored in .jj/repo/config.toml").dim()
    ))?;
    term.write_line("")?;

    let (detected_host, detected_project) = detect_from_remote(&repo_path)?;

    let gitlab_host = if let Some(host) = &detected_host {
        Input::<String>::new()
            .with_prompt(format!("{}", style("GitLab instance URL").bold()))
            .default(host.clone())
            .interact_text()?
    } else {
        Input::<String>::new()
            .with_prompt(format!(
                "{}",
                style("GitLab instance URL (e.g. https://gitlab.example.com)").bold()
            ))
            .interact_text()?
    };

    let gitlab_project = if let Some(project) = &detected_project {
        Input::<String>::new()
            .with_prompt(format!("{}", style("GitLab project ID").bold()))
            .default(project.clone())
            .interact_text()?
    } else {
        term.write_line(&format!("{}", style("Project ID can be either:").dim()))?;
        term.write_line(&format!(
            "{}",
            style("  - Group/project path (e.g., my-group/my-project)").dim()
        ))?;
        term.write_line(&format!(
            "{}",
            style("  - Numeric project ID (e.g., 12345)").dim()
        ))?;

        Input::<String>::new()
            .with_prompt(format!("{}", style("GitLab project ID").bold()))
            .interact_text()?
    };

    term.write_line("")?;
    term.write_line(&format!(
        "{}",
        style("Personal Access Token required scopes:").yellow()
    ))?;
    term.write_line(&format!(
        "  {} {}",
        style("•").yellow(),
        style("api (for creating/updating merge requests)").dim()
    ))?;
    term.write_line("")?;
    term.write_line(&format!(
        "{} {}",
        style("⚠").yellow(),
        style("Note: GitLab does not offer more granular scopes for MR operations.").dim()
    ))?;
    term.write_line(&format!(
        "  {}",
        style("The 'api' scope grants full read/write API access.").dim()
    ))?;
    term.write_line(&format!(
        "  {}",
        style(format!(
            "Create token at: {}/-/user_settings/personal_access_tokens",
            gitlab_host
        ))
        .dim()
    ))?;
    term.write_line("")?;

    let gitlab_token = Password::new()
        .with_prompt(format!("{}", style("GitLab Personal Access Token").bold()))
        .interact()?;

    let branch_prefix = Input::<String>::new()
        .with_prompt(format!(
            "{}",
            style("Branch prefix (e.g. mrs/, leave empty for no prefix)").bold()
        ))
        .default("".to_string())
        .interact_text()?;

    let remote_name = Input::<String>::new()
        .with_prompt(format!("{}", style("Remote name").bold()))
        .default("origin".to_string())
        .interact_text()?;

    let default_branch = Input::<String>::new()
        .with_prompt(format!("{}", style("Default branch").bold()))
        .default("main".to_string())
        .interact_text()?;

    set_config(&repo_path, "spr.gitlabHost", &gitlab_host)?;
    set_config(&repo_path, "spr.gitlabProject", &gitlab_project)?;
    set_config(&repo_path, "spr.gitlabToken", &gitlab_token)?;
    set_config(&repo_path, "spr.branchPrefix", &branch_prefix)?;
    set_config(&repo_path, "spr.remoteName", &remote_name)?;
    set_config(&repo_path, "spr.defaultBranch", &default_branch)?;

    term.write_line("")?;
    term.write_line(&format!(
        "{} {}",
        style("✓").green().bold(),
        style("Configuration complete!").green()
    ))?;
    term.write_line(&format!(
        "{}",
        style("You can now use: jj mr submit <bookmark>").cyan()
    ))?;

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
    for line in remote_output.lines() {
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
