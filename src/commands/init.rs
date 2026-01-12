use std::path::Path;

use dialoguer::{Input, Password, Select};
use owo_colors::OwoColorize;

use crate::{cli::CliConfig, config::ForgeType, error::Result, jj::jj_exec};

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

    let existing_forge: Option<ForgeType> =
        get_config(&cli_config.repository, "jj-vine.forge").and_then(|s| s.parse().ok());

    let detected = detect_forge_from_remote(&cli_config.repository)?;

    let forge_type = if let Some(existing) = existing_forge {
        existing
    } else if let Some(ref detected_forge) = detected {
        detected_forge.forge_type.clone()
    } else {
        let selection = Select::new()
            .with_prompt(format!(
                "{}",
                "[jj-vine.forge] Which forge are you using?".bold()
            ))
            .items(["GitLab", "GitHub", "Forgejo"])
            .default(0)
            .interact()?;

        match selection {
            0 => ForgeType::GitLab,
            1 => ForgeType::GitHub,
            2 => ForgeType::Forgejo,
            _ => ForgeType::GitLab,
        }
    };

    set_config(
        &cli_config.repository,
        "jj-vine.forge",
        forge_type.to_string(),
    )?;

    let existing_remote_name = get_config(&cli_config.repository, "jj-vine.remoteName");
    let remote_name = Input::<String>::new()
        .with_prompt(format!("{}", "[jj-vine.remoteName] Remote name".bold()))
        .default(existing_remote_name.unwrap_or_else(|| "origin".to_string()))
        .interact_text()?;

    let existing_default_branch = get_config(&cli_config.repository, "jj-vine.defaultBranch");
    let default_branch = Input::<String>::new()
        .with_prompt(format!(
            "{}",
            "[jj-vine.defaultBranch] Default branch".bold()
        ))
        .default(existing_default_branch.unwrap_or_else(|| "main".to_string()))
        .interact_text()?;

    set_config(&cli_config.repository, "jj-vine.remoteName", &remote_name)?;
    set_config(
        &cli_config.repository,
        "jj-vine.defaultBranch",
        &default_branch,
    )?;

    match forge_type {
        ForgeType::GitLab => {
            init_gitlab(&cli_config.repository, detected.as_ref()).await?;
        }
        ForgeType::GitHub => {
            init_github(&cli_config.repository, detected.as_ref()).await?;
        }
        ForgeType::Forgejo => {
            init_forgejo(&cli_config.repository, detected.as_ref()).await?;
        }
    }

    println!();
    println!(
        "{} {}",
        "✓".green().bold(),
        "Configuration complete! You can now use: jj mr submit".green()
    );

    Ok(())
}

/// Initialize GitLab-specific configuration
async fn init_gitlab(repo_path: impl AsRef<Path>, detected: Option<&DetectedForge>) -> Result<()> {
    let existing_host = get_config(&repo_path, "jj-vine.gitlab.host");
    let existing_project = get_config(&repo_path, "jj-vine.gitlab.project");
    let existing_token = get_config(&repo_path, "jj-vine.gitlab.token");

    let (detected_host, detected_project) = if let Some(d) = detected {
        if d.forge_type == ForgeType::GitLab {
            (Some(d.host.clone()), Some(d.project.clone()))
        } else {
            (None, None)
        }
    } else {
        (None, None)
    };

    let default_host = existing_host.or(detected_host);
    let default_project = existing_project.or(detected_project);

    let gitlab_host = if let Some(host) = default_host {
        Input::<String>::new()
            .with_prompt(format!(
                "{}",
                "[jj-vine.gitlab.host] GitLab instance URL (e.g. https://gitlab.example.com)"
                    .bold()
            ))
            .default(host)
            .interact_text()?
    } else {
        Input::<String>::new()
            .with_prompt(format!(
                "{}",
                "[jj-vine.gitlab.host] GitLab instance URL (e.g. https://gitlab.example.com)"
                    .bold()
            ))
            .interact_text()?
    };

    let gitlab_project = if let Some(project) = default_project {
        Input::<String>::new()
            .with_prompt(format!(
                "{}",
                "[jj-vine.gitlab.project] GitLab project ID (e.g. group/project)".bold()
            ))
            .default(project)
            .interact_text()?
    } else {
        Input::<String>::new()
            .with_prompt(format!(
                "{}",
                "[jj-vine.gitlab.project] GitLab project ID (e.g. group/project)".bold()
            ))
            .interact_text()?
    };

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
            format!(
                "Create token at: {}/-/user_settings/personal_access_tokens",
                gitlab_host
            )
            .dimmed()
        );
        println!();

        Password::new()
            .with_prompt(format!(
                "{}",
                "[jj-vine.gitlab.token] GitLab Personal Access Token".bold()
            ))
            .interact()?
    };

    set_config(&repo_path, "jj-vine.gitlab.host", &gitlab_host)?;
    set_config(&repo_path, "jj-vine.gitlab.project", &gitlab_project)?;
    set_config(&repo_path, "jj-vine.gitlab.token", &gitlab_token)?;

    Ok(())
}

/// Initialize GitHub-specific configuration
async fn init_github(repo_path: impl AsRef<Path>, detected: Option<&DetectedForge>) -> Result<()> {
    let existing_host = get_config(&repo_path, "jj-vine.github.host");
    let existing_project = get_config(&repo_path, "jj-vine.github.project");
    let existing_token = get_config(&repo_path, "jj-vine.github.token");

    let (detected_host, detected_project) = if let Some(d) = detected {
        if d.forge_type == ForgeType::GitHub {
            (Some(d.host.clone()), Some(d.project.clone()))
        } else {
            (None, None)
        }
    } else {
        (None, None)
    };

    let default_host = existing_host
        .or(detected_host)
        .unwrap_or_else(|| "https://api.github.com".to_string());
    let default_project = existing_project.or(detected_project);

    let github_host = Input::<String>::new()
        .with_prompt(format!(
            "{}",
            "[jj-vine.github.host] GitHub API URL (e.g. https://api.github.com)".bold()
        ))
        .default(default_host)
        .interact_text()?;

    let github_project = if let Some(project) = default_project {
        Input::<String>::new()
            .with_prompt(format!(
                "{}",
                "[jj-vine.github.project] GitHub repository (owner/repo)".bold()
            ))
            .default(project)
            .interact_text()?
    } else {
        Input::<String>::new()
            .with_prompt(format!(
                "{}",
                "[jj-vine.github.project] GitHub repository (owner/repo)".bold()
            ))
            .interact_text()?
    };

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
                "{}",
                "[jj-vine.github.token] GitHub Personal Access Token".bold()
            ))
            .interact()?
    };

    set_config(&repo_path, "jj-vine.github.host", &github_host)?;
    set_config(&repo_path, "jj-vine.github.project", &github_project)?;
    set_config(&repo_path, "jj-vine.github.token", &github_token)?;

    Ok(())
}

/// Initialize Forgejo/Gitea-specific configuration
async fn init_forgejo(repo_path: impl AsRef<Path>, detected: Option<&DetectedForge>) -> Result<()> {
    let existing_host = get_config(&repo_path, "jj-vine.forgejo.host");
    let existing_project = get_config(&repo_path, "jj-vine.forgejo.project");
    let existing_token = get_config(&repo_path, "jj-vine.forgejo.token");

    let (detected_host, detected_project) = if let Some(d) = detected {
        if d.forge_type == ForgeType::Forgejo {
            (Some(d.host.clone()), Some(d.project.clone()))
        } else {
            (None, None)
        }
    } else {
        (None, None)
    };

    let default_host = existing_host
        .or(detected_host)
        .unwrap_or_else(|| "https://codeberg.org".to_string());
    let default_project = existing_project.or(detected_project);

    let forgejo_host = Input::<String>::new()
        .with_prompt(format!(
            "{}",
            "[jj-vine.forgejo.host] Forgejo/Gitea instance URL (e.g. https://codeberg.org)".bold()
        ))
        .default(default_host)
        .interact_text()?;

    let forgejo_project = if let Some(project) = default_project {
        Input::<String>::new()
            .with_prompt(format!(
                "{}",
                "[jj-vine.forgejo.project] Forgejo/Gitea repository (owner/repo)".bold()
            ))
            .default(project)
            .interact_text()?
    } else {
        Input::<String>::new()
            .with_prompt(format!(
                "{}",
                "[jj-vine.forgejo.project] Forgejo/Gitea repository (owner/repo)".bold()
            ))
            .interact_text()?
    };

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
                "{}",
                "[jj-vine.forgejo.token] Forgejo/Gitea Personal Access Token".bold()
            ))
            .interact()?
    };

    set_config(&repo_path, "jj-vine.forgejo.host", &forgejo_host)?;
    set_config(&repo_path, "jj-vine.forgejo.project", &forgejo_project)?;
    set_config(&repo_path, "jj-vine.forgejo.token", &forgejo_token)?;

    Ok(())
}

/// Get a configuration value using jj config get
fn get_config(repo_path: impl AsRef<Path>, key: &str) -> Option<String> {
    match jj_exec(repo_path.as_ref(), ["config", "get", key]) {
        Ok(output) => {
            let value = output.stdout.trim();
            if value.is_empty() {
                None
            } else {
                Some(value.to_string())
            }
        }
        Err(_) => None,
    }
}

/// Set a configuration value using jj config set
fn set_config(repo_path: impl AsRef<Path>, key: &str, value: impl AsRef<str>) -> Result<()> {
    jj_exec(repo_path, ["config", "set", "--repo", key, value.as_ref()])?;
    Ok(())
}

/// Detect forge type, host, and project from git remote
fn detect_forge_from_remote(repo_path: impl AsRef<Path>) -> Result<Option<DetectedForge>> {
    // Get the origin remote URL
    let remote_output = match jj_exec(repo_path, ["git", "remote", "list"]) {
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
    // SSH format: ssh://git@host:owner/repo.git
    // or ssh://git@host/owner/repo.git
    if url.starts_with("git@") || url.starts_with("ssh://git@") {
        let rest = url.trim_start_matches("ssh://").strip_prefix("git@")?;

        let (host, rest) = match rest.split_once(':') {
            Some((host, rest)) => (host, rest),
            None => {
                let (host, rest) = rest.split_once('/')?;
                (host, rest)
            }
        };

        let project = rest.trim_end_matches(".git");
        let forge_type = ForgeType::detect_from_host(host)?;
        let api_host = match forge_type {
            ForgeType::GitHub if host == "github.com" => "https://api.github.com".to_string(),
            ForgeType::GitHub => format!("https://{}/api/v3", host),
            ForgeType::GitLab => format!("https://{}", host),
            ForgeType::Forgejo => format!("https://{}", host),
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

        let forge_type = ForgeType::detect_from_host(host)?;
        let api_host = match forge_type {
            ForgeType::GitHub if host == "github.com" => "https://api.github.com".to_string(),
            ForgeType::GitHub => format!("{}://{}/api/v3", protocol, host),
            ForgeType::GitLab => format!("{}://{}", protocol, host),
            ForgeType::Forgejo => format!("{}://{}", protocol, host),
        };

        return Some(DetectedForge {
            forge_type,
            host: api_host,
            project: project.to_string(),
        });
    }

    None
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
            ForgeType::detect_from_host("github.com"),
            Some(ForgeType::GitHub)
        );
        assert_eq!(
            ForgeType::detect_from_host("github.example.com"),
            Some(ForgeType::GitHub)
        );
    }

    #[test]
    fn test_detect_forge_from_host_gitlab() {
        assert_eq!(
            ForgeType::detect_from_host("gitlab.com"),
            Some(ForgeType::GitLab)
        );
        assert_eq!(
            ForgeType::detect_from_host("gitlab.example.com"),
            Some(ForgeType::GitLab)
        );
    }

    #[test]
    fn test_detect_forge_from_host_unknown() {
        assert_eq!(ForgeType::detect_from_host("git.example.com"), None);
        assert_eq!(ForgeType::detect_from_host("code.example.com"), None);
    }
}
