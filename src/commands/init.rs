use std::{collections::HashMap, path::PathBuf};

use dialoguer::{Input, Select};
use itertools::Itertools;
use owo_colors::OwoColorize;
use serde::Deserialize;
use strum::VariantArray;

use crate::{cli::CliConfig, config::ForgeType, error::Result, jj::Jujutsu};

mod azure;
mod forgejo;
mod github;
mod gitlab;

#[derive(Debug, Clone)]
struct DetectedForge {
    forge_type: ForgeType,
    host: String,
    project: String,

    /// Only for Azure DevOps, the name of the repository.
    repository_name: Option<String>,
}

#[derive(Debug, Clone)]
struct ForkDetection {
    target_project: Option<String>,
}

/// Initialize jj-vine configuration for this repository
pub async fn init(cli_config: &CliConfig<'_>) -> Result<()> {
    println!("This will configure jj-vine for your repository.");
    println!(
        "{}",
        "Configuration will be stored in .jj/repo/config.toml".dimmed()
    );
    println!();

    let existing_forge: Option<ForgeType> =
        get_config(&cli_config.repository, "jj-vine.forge").and_then(|s| s.parse().ok());

    let detected = detect_forge_from_remote(&cli_config.repository)?;
    let fork_detection = detect_fork_setup(&cli_config.repository)?;

    let forge_type = if let Some(existing) = existing_forge {
        existing
    } else if let Some(ref detected_forge) = detected {
        detected_forge.forge_type
    } else {
        let selection = Select::new()
            .with_prompt(format!(
                "{} {}",
                "Which forge are you using?".bold(),
                "jj-vine.forge".dimmed()
            ))
            .items(ForgeType::VARIANTS.iter().map(|v| v.display_name()))
            .default(0)
            .interact()?;

        ForgeType::VARIANTS[selection]
    };

    set_config(
        &cli_config.repository,
        "jj-vine.forge",
        forge_type.to_string(),
    )?;

    let existing_remote_name = get_config(&cli_config.repository, "jj-vine.remoteName");
    let remote_name = Input::<String>::new()
        .with_prompt(format!(
            "{} {}",
            "Remote name".bold(),
            "jj-vine.remoteName".dimmed()
        ))
        .default(existing_remote_name.unwrap_or_else(|| "origin".to_string()))
        .interact_text()?;

    let existing_default_branch = get_config(&cli_config.repository, "jj-vine.defaultBranch");
    let default_branch = Input::<String>::new()
        .with_prompt(format!(
            "{} {}",
            "Default branch".bold(),
            "jj-vine.defaultBranch".dimmed()
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
            gitlab::init(
                &cli_config.repository,
                detected.as_ref(),
                fork_detection.as_ref(),
            )
            .await?;
        }
        ForgeType::GitHub => {
            github::init(
                &cli_config.repository,
                detected.as_ref(),
                fork_detection.as_ref(),
            )
            .await?;
        }
        ForgeType::Forgejo => {
            forgejo::init(
                &cli_config.repository,
                detected.as_ref(),
                fork_detection.as_ref(),
            )
            .await?;
        }
        ForgeType::AzureDevOps => {
            azure::init(
                &cli_config.repository,
                detected.as_ref(),
                fork_detection.as_ref(),
            )
            .await?;
        }
    }

    let jj = Jujutsu::new(&cli_config.repository)?;

    let recommended_alias = match forge_type {
        ForgeType::GitLab => "pr",
        ForgeType::GitHub => "mr",
        ForgeType::Forgejo => "pr",
        ForgeType::AzureDevOps => "pr",
    };
    let recommendation = format!(
        "\nIt is useful to set up an alias for this command, such as {}! Run {} to set it up.",
        format!("jj {}", recommended_alias).bold().magenta(),
        format!(
            r#"jj config set --user aliases.{} '["util", "exec", "--", "jj-vine"]'"#,
            recommended_alias
        )
        .cyan()
    );

    let message = match toml::from_str::<JJConfig>(&jj.exec(["config", "list", "aliases"])?.stdout)
    {
        Ok(JJConfig {
            aliases: Some(aliases),
            ..
        }) => {
            let alias = aliases
                .iter()
                .find(|(_, args)| args.iter().any(|a| a.contains("jj-vine")));
            match alias {
                Some((alias, _)) => format!(
                    "Configuration complete! You can now use: {}.",
                    format!("jj {}", alias).bold()
                )
                .green()
                .to_string(),
                None => format!(
                    "Configuration complete! You can now use: {}.{}",
                    "jj-vine submit".bold(),
                    recommendation
                )
                .green()
                .to_string(),
            }
        }
        _ => format!(
            "Configuration complete! You can now use: {}.{}",
            "jj-vine submit".bold(),
            recommendation
        )
        .green()
        .to_string(),
    };

    println!(
        "\n{} {}\n\nThere are more configuration options available. See all configuration options at https://codeberg.org/abrenneke/jj-vine#configuration.",
        "✓".green().bold(),
        message
    );

    Ok(())
}

#[derive(Debug, Clone, Deserialize)]
struct JJConfig {
    aliases: Option<HashMap<String, Vec<String>>>,
}

/// Get a configuration value using jj config get
fn get_config(repo_path: impl Into<PathBuf>, key: &str) -> Option<String> {
    match Jujutsu::new(repo_path).ok()?.exec(["config", "get", key]) {
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
fn set_config(repo_path: impl Into<PathBuf>, key: &str, value: impl AsRef<str>) -> Result<()> {
    Jujutsu::new(repo_path)?.exec(["config", "set", "--repo", key, value.as_ref()])?;
    Ok(())
}

/// Detect forge type, host, and project from git remote
fn detect_forge_from_remote(repo_path: impl Into<PathBuf>) -> Result<Option<DetectedForge>> {
    // Get the origin remote URL
    let remote_output = match Jujutsu::new(repo_path)?.exec(["git", "remote", "list"]) {
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

/// Detect fork setup from git remotes
fn detect_fork_setup(repo_path: impl Into<PathBuf>) -> Result<Option<ForkDetection>> {
    let remote_output = match Jujutsu::new(repo_path)?.exec(["git", "remote", "list"]) {
        Ok(output) => output,
        Err(_) => return Ok(None),
    };

    let mut remotes: Vec<(String, String)> = Vec::new();
    for line in remote_output.stdout.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            remotes.push((parts[0].to_string(), parts[1].to_string()));
        }
    }

    // Look for origin + upstream pattern
    let origin = remotes.iter().find(|(name, _)| name == "origin");
    let upstream = remotes.iter().find(|(name, _)| name == "upstream");

    if let (Some((_, origin_url)), Some((_, upstream_url))) = (origin, upstream)
        && let (Some(_), Some(target)) = (
            parse_project_from_url(origin_url),
            parse_project_from_url(upstream_url),
        )
    {
        return Ok(Some(ForkDetection {
            target_project: Some(target),
        }));
    }

    // Look for fork + origin pattern (reverse convention)
    let fork = remotes.iter().find(|(name, _)| name == "fork");
    if let (Some((_, fork_url)), Some((_, origin_url))) = (fork, origin)
        && let (Some(_), Some(target)) = (
            parse_project_from_url(fork_url),
            parse_project_from_url(origin_url),
        )
    {
        return Ok(Some(ForkDetection {
            target_project: Some(target),
        }));
    }

    // No fork detected - just return origin if it exists
    if let Some((_, origin_url)) = origin
        && parse_project_from_url(origin_url).is_some()
    {
        return Ok(Some(ForkDetection {
            target_project: None,
        }));
    }

    Ok(None)
}

/// Extract project path from a git remote URL
fn parse_project_from_url(url: &str) -> Option<String> {
    // SSH format: git@host:owner/repo.git or ssh://git@host/owner/repo.git
    if url.starts_with("git@") || url.starts_with("ssh://git@") {
        let rest = url.trim_start_matches("ssh://").strip_prefix("git@")?;

        let project = match rest.split_once(':') {
            Some((_, rest)) => rest,
            None => {
                let (_, rest) = rest.split_once('/')?;
                rest
            }
        };

        return Some(project.trim_end_matches(".git").to_string());
    }

    // HTTPS format: https://host/owner/repo.git
    if url.starts_with("https://") || url.starts_with("http://") {
        let without_protocol = url
            .strip_prefix("https://")
            .or_else(|| url.strip_prefix("http://"))?;

        let (_, path) = without_protocol.split_once('/')?;
        let project = path.strip_suffix(".git").unwrap_or(path);

        return Some(project.to_string());
    }

    None
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

        let forge_type = ForgeType::detect_from_host(host)?;

        let (project, repository_name) = match forge_type {
            ForgeType::AzureDevOps => {
                if let Some((_, org, project, repo)) =
                    rest.trim_end_matches(".git").split('/').collect_tuple()
                {
                    (format!("{}/{}", org, project), Some(repo.to_string()))
                } else {
                    (rest.trim_end_matches(".git").to_string(), None)
                }
            }
            _ => (rest.trim_end_matches(".git").to_string(), None),
        };

        let api_host = match forge_type {
            ForgeType::GitHub if host == "github.com" => "https://api.github.com".to_string(),
            ForgeType::GitHub => format!("https://{}/api/v3", host),
            ForgeType::GitLab => format!("https://{}", host),
            ForgeType::Forgejo => format!("https://{}", host),
            ForgeType::AzureDevOps => format!("https://{}", host),
        };

        return Some(DetectedForge {
            forge_type,
            host: api_host,
            project: project.to_string(),
            repository_name,
        });
    }

    // HTTPS format: https://host/owner/repo.git
    if url.starts_with("https://") || url.starts_with("http://") {
        let without_protocol = url
            .strip_prefix("https://")
            .or_else(|| url.strip_prefix("http://"))?;

        let (host, path) = without_protocol.split_once('/')?;

        let protocol = if url.starts_with("https://") {
            "https"
        } else {
            "http"
        };

        let forge_type = ForgeType::detect_from_host(host)?;

        let (project, repository_name) = match forge_type {
            ForgeType::AzureDevOps => {
                if let Some((_, org, project, repo)) =
                    path.trim_end_matches(".git").split('/').collect_tuple()
                {
                    (format!("{}/{}", org, project), Some(repo.to_string()))
                } else {
                    (path.trim_end_matches(".git").to_string(), None)
                }
            }
            _ => (path.trim_end_matches(".git").to_string(), None),
        };

        let api_host = match forge_type {
            ForgeType::GitHub if host == "github.com" => "https://api.github.com".to_string(),
            ForgeType::GitHub => format!("{}://{}/api/v3", protocol, host),
            ForgeType::GitLab => format!("{}://{}", protocol, host),
            ForgeType::Forgejo => format!("{}://{}", protocol, host),
            ForgeType::AzureDevOps => format!("{}://{}", protocol, host),
        };

        return Some(DetectedForge {
            forge_type,
            host: api_host,
            project: project.to_string(),
            repository_name,
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
