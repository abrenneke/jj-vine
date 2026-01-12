use std::path::PathBuf;

use dialoguer::{Input, Password, Select};
use owo_colors::OwoColorize;

use crate::{cli::CliConfig, config::ForgeType, error::Result, jj::run_jj_command};

#[derive(Debug, Clone)]
struct DetectedForge {
    forge_type: ForgeType,
    host: String,
    project: String,
}

/// Initialize jj-vine configuration for this repository
pub async fn init(cli_config: CliConfig<'_>) -> Result<()> {
    println!("This will configure jj-vine for your repository.");
    println!(
        "{}",
        "Configuration will be stored in .jj/repo/config.toml".dimmed()
    );
    println!();

    // Try to detect forge from remote
    let detected = detect_forge_from_remote(&cli_config.repository)?;

    let forge_type = if let Some(ref detected_forge) = detected {
        println!(
            "{} Detected {} repository",
            "✓".green(),
            match detected_forge.forge_type {
                ForgeType::GitLab => "GitLab",
                ForgeType::GitHub => "GitHub",
            }
        );
        println!();
        detected_forge.forge_type.clone()
    } else {
        println!("Could not detect forge from git remote.");
        println!();
        let selection = Select::new()
            .with_prompt(format!("{}", "Which forge are you using?".bold()))
            .items(["GitLab", "GitHub"])
            .default(0)
            .interact()?;

        match selection {
            0 => ForgeType::GitLab,
            1 => ForgeType::GitHub,
            _ => ForgeType::GitLab,
        }
    };

    set_config(
        &cli_config.repository,
        "jj-vine.forge",
        forge_type.to_string(),
    )?;

    // Get common configuration
    let remote_name = Input::<String>::new()
        .with_prompt(format!("{}", "Remote name".bold()))
        .default("origin".to_string())
        .interact_text()?;

    let default_branch = Input::<String>::new()
        .with_prompt(format!("{}", "Default branch".bold()))
        .default("main".to_string())
        .interact_text()?;

    set_config(&cli_config.repository, "jj-vine.remoteName", &remote_name)?;
    set_config(
        &cli_config.repository,
        "jj-vine.defaultBranch",
        &default_branch,
    )?;

    // Delegate to forge-specific init
    match forge_type {
        ForgeType::GitLab => {
            init_gitlab(&cli_config.repository, detected.as_ref()).await?;
        }
        ForgeType::GitHub => {
            init_github(&cli_config.repository, detected.as_ref()).await?;
        }
    }

    println!();
    println!(
        "{} {}",
        "✓".green().bold(),
        "Configuration complete!".green()
    );
    println!("{}", "You can now use: jj mr submit".cyan());

    Ok(())
}

/// Initialize GitLab-specific configuration
async fn init_gitlab(repo_path: &PathBuf, detected: Option<&DetectedForge>) -> Result<()> {
    let (detected_host, detected_project) = if let Some(d) = detected {
        if d.forge_type == ForgeType::GitLab {
            (Some(d.host.clone()), Some(d.project.clone()))
        } else {
            (None, None)
        }
    } else {
        (None, None)
    };

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

    set_config(repo_path, "jj-vine.gitlab.host", &gitlab_host)?;
    set_config(repo_path, "jj-vine.gitlab.project", &gitlab_project)?;
    set_config(repo_path, "jj-vine.gitlab.token", &gitlab_token)?;

    Ok(())
}

/// Initialize GitHub-specific configuration
async fn init_github(repo_path: &PathBuf, detected: Option<&DetectedForge>) -> Result<()> {
    let (detected_host, detected_project) = if let Some(d) = detected {
        if d.forge_type == ForgeType::GitHub {
            (Some(d.host.clone()), Some(d.project.clone()))
        } else {
            (None, None)
        }
    } else {
        (None, None)
    };

    let github_host = if let Some(host) = &detected_host {
        Input::<String>::new()
            .with_prompt(format!("{}", "GitHub API URL".bold()))
            .default(host.clone())
            .interact_text()?
    } else {
        Input::<String>::new()
            .with_prompt(format!("{}", "GitHub API URL".bold()))
            .default("https://api.github.com".to_string())
            .interact_text()?
    };

    let github_project = if let Some(project) = &detected_project {
        Input::<String>::new()
            .with_prompt(format!("{}", "GitHub repository (owner/repo)".bold()))
            .default(project.clone())
            .interact_text()?
    } else {
        println!(
            "{}",
            "Repository format: owner/repo (e.g., torvalds/linux)".dimmed()
        );

        Input::<String>::new()
            .with_prompt(format!("{}", "GitHub repository".bold()))
            .interact_text()?
    };

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

    let github_token = Password::new()
        .with_prompt(format!("{}", "GitHub Personal Access Token".bold()))
        .interact()?;

    set_config(repo_path, "jj-vine.github.host", &github_host)?;
    set_config(repo_path, "jj-vine.github.project", &github_project)?;
    set_config(repo_path, "jj-vine.github.token", &github_token)?;

    Ok(())
}

/// Set a configuration value using jj config set
fn set_config(repo_path: &PathBuf, key: &str, value: impl AsRef<str>) -> Result<()> {
    run_jj_command(repo_path, ["config", "set", "--repo", key, value.as_ref()])?;
    Ok(())
}

/// Detect forge type, host, and project from git remote
fn detect_forge_from_remote(repo_path: &PathBuf) -> Result<Option<DetectedForge>> {
    // Get the origin remote URL
    let remote_output = match run_jj_command(repo_path, ["git", "remote", "list"]) {
        Ok(output) => output,
        Err(_) => return Ok(None),
    };

    // Parse the first remote (typically origin)
    for line in remote_output.stdout.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            let url = parts[1];

            // Try to detect forge from URL
            if let Some(detected) = parse_forge_url(url) {
                return Ok(Some(detected));
            }
        }
    }

    Ok(None)
}

/// Parse a forge remote URL to detect forge type, host, and project
fn parse_forge_url(url: &str) -> Option<DetectedForge> {
    // SSH format: git@host:owner/repo.git
    if url.starts_with("git@") {
        let rest = url.strip_prefix("git@")?;
        let (host, path) = rest.split_once(':')?;
        let project = path.strip_suffix(".git").unwrap_or(path);

        let forge_type = detect_forge_from_host(host)?;
        let api_host = match forge_type {
            ForgeType::GitHub if host == "github.com" => "https://api.github.com".to_string(),
            ForgeType::GitHub => format!("https://{}/api/v3", host),
            ForgeType::GitLab => format!("https://{}", host),
        };

        return Some(DetectedForge {
            forge_type,
            host: api_host,
            project: project.to_string(),
        });
    }

    // HTTPS format: https://host/owner/repo.git
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

        let forge_type = detect_forge_from_host(host)?;
        let api_host = match forge_type {
            ForgeType::GitHub if host == "github.com" => "https://api.github.com".to_string(),
            ForgeType::GitHub => format!("{}://{}/api/v3", protocol, host),
            ForgeType::GitLab => format!("{}://{}", protocol, host),
        };

        return Some(DetectedForge {
            forge_type,
            host: api_host,
            project: project.to_string(),
        });
    }

    None
}

/// Detect forge type from hostname
fn detect_forge_from_host(host: &str) -> Option<ForgeType> {
    if host.contains("github") {
        Some(ForgeType::GitHub)
    } else if host.contains("gitlab") {
        Some(ForgeType::GitLab)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_gitlab_url_ssh() {
        let url = "git@gitlab.example.com:group/project.git";
        let result = parse_forge_url(url);
        assert!(result.is_some());
        let detected = result.unwrap();
        assert_eq!(detected.forge_type, ForgeType::GitLab);
        assert_eq!(detected.host, "https://gitlab.example.com");
        assert_eq!(detected.project, "group/project");
    }

    #[test]
    fn test_parse_gitlab_url_https() {
        let url = "https://gitlab.example.com/group/project.git";
        let result = parse_forge_url(url);
        assert!(result.is_some());
        let detected = result.unwrap();
        assert_eq!(detected.forge_type, ForgeType::GitLab);
        assert_eq!(detected.host, "https://gitlab.example.com");
        assert_eq!(detected.project, "group/project");
    }

    #[test]
    fn test_parse_gitlab_url_nested_groups() {
        let url = "git@gitlab.example.com:group/subgroup/project.git";
        let result = parse_forge_url(url);
        assert!(result.is_some());
        let detected = result.unwrap();
        assert_eq!(detected.forge_type, ForgeType::GitLab);
        assert_eq!(detected.project, "group/subgroup/project");
    }

    #[test]
    fn test_parse_github_url_ssh() {
        let url = "git@github.com:owner/repo.git";
        let result = parse_forge_url(url);
        assert!(result.is_some());
        let detected = result.unwrap();
        assert_eq!(detected.forge_type, ForgeType::GitHub);
        assert_eq!(detected.host, "https://api.github.com");
        assert_eq!(detected.project, "owner/repo");
    }

    #[test]
    fn test_parse_github_url_https() {
        let url = "https://github.com/owner/repo.git";
        let result = parse_forge_url(url);
        assert!(result.is_some());
        let detected = result.unwrap();
        assert_eq!(detected.forge_type, ForgeType::GitHub);
        assert_eq!(detected.host, "https://api.github.com");
        assert_eq!(detected.project, "owner/repo");
    }

    #[test]
    fn test_parse_github_enterprise_ssh() {
        let url = "git@github.example.com:owner/repo.git";
        let result = parse_forge_url(url);
        assert!(result.is_some());
        let detected = result.unwrap();
        assert_eq!(detected.forge_type, ForgeType::GitHub);
        assert_eq!(detected.host, "https://github.example.com/api/v3");
        assert_eq!(detected.project, "owner/repo");
    }

    #[test]
    fn test_parse_github_enterprise_https() {
        let url = "https://github.example.com/owner/repo.git";
        let result = parse_forge_url(url);
        assert!(result.is_some());
        let detected = result.unwrap();
        assert_eq!(detected.forge_type, ForgeType::GitHub);
        assert_eq!(detected.host, "https://github.example.com/api/v3");
        assert_eq!(detected.project, "owner/repo");
    }

    #[test]
    fn test_detect_forge_from_host_github() {
        assert_eq!(
            detect_forge_from_host("github.com"),
            Some(ForgeType::GitHub)
        );
        assert_eq!(
            detect_forge_from_host("github.example.com"),
            Some(ForgeType::GitHub)
        );
    }

    #[test]
    fn test_detect_forge_from_host_gitlab() {
        assert_eq!(
            detect_forge_from_host("gitlab.com"),
            Some(ForgeType::GitLab)
        );
        assert_eq!(
            detect_forge_from_host("gitlab.example.com"),
            Some(ForgeType::GitLab)
        );
    }

    #[test]
    fn test_detect_forge_from_host_unknown() {
        assert_eq!(detect_forge_from_host("git.example.com"), None);
        assert_eq!(detect_forge_from_host("code.example.com"), None);
    }
}
