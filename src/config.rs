use std::path::PathBuf;

use serde::Deserialize;

use crate::{
    error::{Error, Result},
    jj::jj_exec,
};

/// Forge type (GitLab, GitHub, or Forgejo)
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ForgeType {
    /// GitLab (GitLab.com or self-hosted)
    GitLab,
    /// GitHub (GitHub.com or GitHub Enterprise)
    GitHub,
    /// Forgejo/Gitea (self-hosted or Codeberg)
    Forgejo,
}

impl std::fmt::Display for ForgeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                ForgeType::GitLab => "gitlab",
                ForgeType::GitHub => "github",
                ForgeType::Forgejo => "forgejo",
            }
        )
    }
}

impl std::str::FromStr for ForgeType {
    type Err = Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "gitlab" => Ok(Self::GitLab),
            "github" => Ok(Self::GitHub),
            "forgejo" => Ok(Self::Forgejo),
            _ => Err(Error::Config {
                message: format!("Invalid forge type: {}", s),
            }),
        }
    }
}

impl ForgeType {
    pub fn detect_from_host(host: &str) -> Option<Self> {
        if host.contains("gitlab") {
            Some(Self::GitLab)
        } else if host.contains("github") {
            Some(Self::GitHub)
        } else if host.contains("forgejo") || host.contains("gitea") || host.contains("codeberg") {
            Some(Self::Forgejo)
        } else {
            None
        }
    }
}

/// Stack visualization format
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StackFormat {
    /// Linear numbered list (default)
    Linear,
    // Future formats: Tree, Compact, Custom(String)
}

fn default_remote_name() -> String {
    "origin".to_string()
}

fn default_branch() -> String {
    "main".to_string()
}

fn default_true() -> bool {
    true
}

fn default_stack_format() -> StackFormat {
    StackFormat::Linear
}

/// Configuration for jj-vine
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    /// Which forge to use (gitlab or github)
    pub forge: ForgeType,

    // ===== Common Configuration =====
    /// Git remote name (default: "origin")
    #[serde(default = "default_remote_name")]
    pub remote_name: String,

    /// Default branch name (default: "main")
    #[serde(default = "default_branch")]
    pub default_branch: String,

    /// Optional path to CA bundle for TLS verification
    #[serde(default)]
    pub ca_bundle: Option<String>,

    /// Accept non-compliant TLS certificates (for certificates that don't meet
    /// strict X.509 standards)
    #[serde(default)]
    pub tls_accept_non_compliant_certs: bool,

    /// Enable stack visualization in MR descriptions (default: true)
    #[serde(default = "default_true")]
    pub enable_stack_visualization: bool,

    /// Stack visualization format (default: Linear)
    #[serde(default = "default_stack_format")]
    pub stack_format: StackFormat,

    /// Delete source branch when MR is merged (default: true)
    #[serde(default = "default_true")]
    pub delete_source_branch: bool,

    /// Squash commits when MR is merged (default: false)
    #[serde(default)]
    pub squash_commits: bool,

    /// Assign created MRs to yourself (default: false)
    #[serde(default)]
    pub assign_to_self: bool,

    /// Default reviewers for created MRs (list of usernames)
    #[serde(default)]
    pub default_reviewers: Vec<String>,

    /// GitLab configuration
    #[serde(default)]
    pub gitlab: GitLabConfig,

    /// GitHub configuration
    #[serde(default)]
    pub github: GitHubConfig,

    /// Forgejo/Gitea configuration
    #[serde(default)]
    pub forgejo: ForgejoConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitLabConfig {
    /// GitLab instance URL (e.g., <https://gitlab.example.com>)
    #[serde(default)]
    pub host: String,

    /// GitLab project ID (e.g., "group/project" or "12345")
    #[serde(default)]
    pub project: String,

    /// GitLab Personal Access Token
    #[serde(default)]
    pub token: String,
}

impl Default for GitLabConfig {
    fn default() -> Self {
        Self {
            host: "".to_string(),
            project: "".to_string(),
            token: "".to_string(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubConfig {
    /// GitHub API URL (e.g., "https://api.github.com" or "https://github.example.com/api/v3")
    #[serde(default)]
    pub host: String,

    /// GitHub repository in "owner/repo" format
    #[serde(default)]
    pub project: String,

    /// GitHub Personal Access Token
    #[serde(default)]
    pub token: String,
}

impl Default for GitHubConfig {
    fn default() -> Self {
        Self {
            host: "".to_string(),
            project: "".to_string(),
            token: "".to_string(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForgejoConfig {
    /// Forgejo/Gitea instance URL (e.g., <https://codeberg.org>)
    #[serde(default)]
    pub host: String,

    /// Repository in "owner/repo" format
    #[serde(default)]
    pub project: String,

    /// API access token
    #[serde(default)]
    pub token: String,
}

impl Default for ForgejoConfig {
    fn default() -> Self {
        Self {
            host: "".to_string(),
            project: "".to_string(),
            token: "".to_string(),
        }
    }
}

impl Config {
    /// Load configuration from jj config
    pub fn load(repo_path: &PathBuf) -> Result<Self> {
        let output = jj_exec(repo_path, ["config", "list"])?;

        let toml_value: toml::Value =
            toml::from_str(&output.stdout).map_err(|e| Error::Config {
                message: format!("Failed to parse config as TOML: {}", e),
            })?;

        let jj_vine_value = toml_value.get("jj-vine").ok_or_else(|| Error::Config {
            message: "Missing required config section: jj-vine".to_string(),
        })?;

        let config: Config = jj_vine_value
            .clone()
            .try_into()
            .map_err(|e| Error::Config {
                message: format!("Failed to parse jj-vine config: {}", e),
            })?;

        config.validate()?;

        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        match self.forge {
            ForgeType::GitLab => {
                if self.gitlab.host.is_empty() {
                    return Err(Error::Config {
                        message: "gitlab.host is required when forge is gitlab".to_string(),
                    });
                }
                if self.gitlab.project.is_empty() {
                    return Err(Error::Config {
                        message: "gitlab.project is required when forge is gitlab".to_string(),
                    });
                }
                if self.gitlab.token.is_empty() {
                    return Err(Error::Config {
                        message: "gitlab.token is required when forge is gitlab".to_string(),
                    });
                }
            }
            ForgeType::GitHub => {
                if self.github.project.is_empty() {
                    return Err(Error::Config {
                        message: "github.project is required when forge is github".to_string(),
                    });
                }
                if self.github.token.is_empty() {
                    return Err(Error::Config {
                        message: "github.token is required when forge is github".to_string(),
                    });
                }
            }
            ForgeType::Forgejo => {
                if self.forgejo.host.is_empty() {
                    return Err(Error::Config {
                        message: "forgejo.host is required when forge is forgejo".to_string(),
                    });
                }
                if self.forgejo.project.is_empty() {
                    return Err(Error::Config {
                        message: "forgejo.project is required when forge is forgejo".to_string(),
                    });
                }
                if self.forgejo.token.is_empty() {
                    return Err(Error::Config {
                        message: "forgejo.token is required when forge is forgejo".to_string(),
                    });
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    fn create_test_repo() -> (TempDir, PathBuf) {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let repo_path = temp_dir.path().to_path_buf();

        // Initialize jj repo
        jj_exec(&repo_path, ["git", "init", "--colocate"]).expect("Failed to init jj repo");

        (temp_dir, repo_path)
    }

    #[test]
    fn test_config_load_missing_required() {
        let (_temp, repo_path) = create_test_repo();

        // Try to load config without setting anything
        let result = Config::load(&repo_path);
        assert!(result.is_err());

        if let Err(Error::Config { message }) = result {
            assert!(
                message.contains("missing field")
                    || message.contains("gitlab")
                    || message.contains("jj-vine"),
                "Error should mention missing field, got: {}",
                message
            );
        } else {
            panic!("Expected Config error for missing required field");
        }
    }

    #[test]
    fn test_config_load_complete() {
        let (_temp, repo_path) = create_test_repo();

        // Set required config
        jj_exec(
            &repo_path,
            [
                "config",
                "set",
                "--repo",
                "jj-vine.gitlab.host",
                "https://gitlab.example.com",
            ],
        )
        .expect("Failed to set config");

        jj_exec(
            &repo_path,
            [
                "config",
                "set",
                "--repo",
                "jj-vine.gitlab.project",
                "my-group/my-project",
            ],
        )
        .expect("Failed to set config");

        jj_exec(
            &repo_path,
            [
                "config",
                "set",
                "--repo",
                "jj-vine.gitlab.token",
                "glpat-test123",
            ],
        )
        .expect("Failed to set config");

        jj_exec(
            &repo_path,
            ["config", "set", "--repo", "jj-vine.forge", "gitlab"],
        )
        .expect("Failed to set config");

        // Load config
        let config = Config::load(&repo_path).expect("Failed to load config");

        assert_eq!(config.gitlab.host, "https://gitlab.example.com".to_string());
        assert_eq!(config.gitlab.project, "my-group/my-project".to_string());
        assert_eq!(config.gitlab.token, "glpat-test123".to_string());
        assert_eq!(config.remote_name, "origin");
        assert_eq!(config.default_branch, "main");
    }

    #[test]
    fn test_config_with_optional_fields() {
        let (_temp, repo_path) = create_test_repo();

        // Set all config including optional fields
        jj_exec(
            &repo_path,
            [
                "config",
                "set",
                "--repo",
                "jj-vine.gitlab.host",
                "https://gitlab.example.com",
            ],
        )
        .expect("Failed to set config");

        jj_exec(
            &repo_path,
            [
                "config",
                "set",
                "--repo",
                "jj-vine.gitlab.project",
                "my-group/my-project",
            ],
        )
        .expect("Failed to set config");

        jj_exec(
            &repo_path,
            [
                "config",
                "set",
                "--repo",
                "jj-vine.gitlab.token",
                "glpat-test123",
            ],
        )
        .expect("Failed to set config");

        jj_exec(
            &repo_path,
            ["config", "set", "--repo", "jj-vine.branchPrefix", "mrs/"],
        )
        .expect("Failed to set config");

        jj_exec(
            &repo_path,
            ["config", "set", "--repo", "jj-vine.remoteName", "upstream"],
        )
        .expect("Failed to set config");

        jj_exec(
            &repo_path,
            ["config", "set", "--repo", "jj-vine.defaultBranch", "master"],
        )
        .expect("Failed to set config");

        jj_exec(
            &repo_path,
            ["config", "set", "--repo", "jj-vine.forge", "gitlab"],
        )
        .expect("Failed to set config");

        // Load config
        let config = Config::load(&repo_path).expect("Failed to load config");

        assert_eq!(config.gitlab.host, "https://gitlab.example.com".to_string());
        assert_eq!(config.gitlab.project, "my-group/my-project".to_string());
        assert_eq!(config.gitlab.token, "glpat-test123".to_string());
        assert_eq!(config.remote_name, "upstream");
        assert_eq!(config.default_branch, "master");
    }

    #[test]
    fn test_config_default_stack_visualization() {
        let (_temp, repo_path) = create_test_repo();

        // Set required config, but not stack visualization config
        jj_exec(
            &repo_path,
            [
                "config",
                "set",
                "--repo",
                "jj-vine.gitlab.host",
                "https://gitlab.com",
            ],
        )
        .expect("Failed to set config");

        jj_exec(
            &repo_path,
            [
                "config",
                "set",
                "--repo",
                "jj-vine.gitlab.project",
                "test/proj",
            ],
        )
        .expect("Failed to set config");

        jj_exec(
            &repo_path,
            ["config", "set", "--repo", "jj-vine.gitlab.token", "token"],
        )
        .expect("Failed to set config");

        jj_exec(
            &repo_path,
            ["config", "set", "--repo", "jj-vine.forge", "gitlab"],
        )
        .expect("Failed to set config");

        let config = Config::load(&repo_path).expect("Failed to load config");

        assert!(config.enable_stack_visualization);
        assert!(matches!(config.stack_format, StackFormat::Linear));
    }

    #[test]
    fn test_config_explicit_stack_visualization() {
        let (_temp, repo_path) = create_test_repo();

        // Set required config
        jj_exec(
            &repo_path,
            [
                "config",
                "set",
                "--repo",
                "jj-vine.gitlab.host",
                "https://gitlab.com",
            ],
        )
        .expect("Failed to set config");

        jj_exec(
            &repo_path,
            [
                "config",
                "set",
                "--repo",
                "jj-vine.gitlab.project",
                "test/proj",
            ],
        )
        .expect("Failed to set config");

        jj_exec(
            &repo_path,
            ["config", "set", "--repo", "jj-vine.gitlab.token", "token"],
        )
        .expect("Failed to set config");

        // Set explicit stack visualization config (disable it)
        jj_exec(
            &repo_path,
            [
                "config",
                "set",
                "--repo",
                "jj-vine.enableStackVisualization",
                "false",
            ],
        )
        .expect("Failed to set config");

        jj_exec(
            &repo_path,
            ["config", "set", "--repo", "jj-vine.forge", "gitlab"],
        )
        .expect("Failed to set config");

        let config = Config::load(&repo_path).expect("Failed to load config");

        assert!(!config.enable_stack_visualization);
        assert!(matches!(config.stack_format, StackFormat::Linear)); // Still default
    }

    #[test]
    fn test_config_default_mr_settings() {
        let (_temp, repo_path) = create_test_repo();

        // Set required config only, don't set MR settings
        jj_exec(
            &repo_path,
            [
                "config",
                "set",
                "--repo",
                "jj-vine.gitlab.host",
                "https://gitlab.com",
            ],
        )
        .expect("Failed to set config");

        jj_exec(
            &repo_path,
            [
                "config",
                "set",
                "--repo",
                "jj-vine.gitlab.project",
                "test/proj",
            ],
        )
        .expect("Failed to set config");

        jj_exec(
            &repo_path,
            ["config", "set", "--repo", "jj-vine.gitlab.token", "token"],
        )
        .expect("Failed to set config");

        jj_exec(
            &repo_path,
            ["config", "set", "--repo", "jj-vine.forge", "gitlab"],
        )
        .expect("Failed to set config");

        let config = Config::load(&repo_path).expect("Failed to load config");

        assert!(config.delete_source_branch);
        assert!(!config.squash_commits);
    }

    #[test]
    fn test_config_explicit_mr_settings() {
        let (_temp, repo_path) = create_test_repo();

        // Set required config
        jj_exec(
            &repo_path,
            [
                "config",
                "set",
                "--repo",
                "jj-vine.gitlab.host",
                "https://gitlab.com",
            ],
        )
        .expect("Failed to set config");

        jj_exec(
            &repo_path,
            [
                "config",
                "set",
                "--repo",
                "jj-vine.gitlab.project",
                "test/proj",
            ],
        )
        .expect("Failed to set config");

        jj_exec(
            &repo_path,
            ["config", "set", "--repo", "jj-vine.gitlab.token", "token"],
        )
        .expect("Failed to set config");

        // Set explicit MR settings (opposite of defaults)
        jj_exec(
            &repo_path,
            [
                "config",
                "set",
                "--repo",
                "jj-vine.deleteSourceBranch",
                "false",
            ],
        )
        .expect("Failed to set config");

        jj_exec(
            &repo_path,
            ["config", "set", "--repo", "jj-vine.squashCommits", "true"],
        )
        .expect("Failed to set config");

        jj_exec(
            &repo_path,
            ["config", "set", "--repo", "jj-vine.forge", "gitlab"],
        )
        .expect("Failed to set config");

        let config = Config::load(&repo_path).expect("Failed to load config");

        assert!(!config.delete_source_branch);
        assert!(config.squash_commits);
    }

    #[test]
    fn test_config_default_assign_to_self() {
        let (_temp, repo_path) = create_test_repo();

        // Set required config only
        jj_exec(
            &repo_path,
            [
                "config",
                "set",
                "--repo",
                "jj-vine.gitlab.host",
                "https://gitlab.com",
            ],
        )
        .expect("Failed to set config");

        jj_exec(
            &repo_path,
            [
                "config",
                "set",
                "--repo",
                "jj-vine.gitlab.project",
                "test/proj",
            ],
        )
        .expect("Failed to set config");

        jj_exec(
            &repo_path,
            ["config", "set", "--repo", "jj-vine.gitlab.token", "token"],
        )
        .expect("Failed to set config");

        jj_exec(
            &repo_path,
            ["config", "set", "--repo", "jj-vine.forge", "gitlab"],
        )
        .expect("Failed to set config");

        let config = Config::load(&repo_path).expect("Failed to load config");

        assert!(!config.assign_to_self);
    }

    #[test]
    fn test_config_explicit_assign_to_self() {
        let (_temp, repo_path) = create_test_repo();

        // Set required config
        jj_exec(
            &repo_path,
            [
                "config",
                "set",
                "--repo",
                "jj-vine.gitlab.host",
                "https://gitlab.com",
            ],
        )
        .expect("Failed to set config");

        jj_exec(
            &repo_path,
            [
                "config",
                "set",
                "--repo",
                "jj-vine.gitlab.project",
                "test/proj",
            ],
        )
        .expect("Failed to set config");

        jj_exec(
            &repo_path,
            ["config", "set", "--repo", "jj-vine.gitlab.token", "token"],
        )
        .expect("Failed to set config");

        // Set assign_to_self to true
        jj_exec(
            &repo_path,
            ["config", "set", "--repo", "jj-vine.assignToSelf", "true"],
        )
        .expect("Failed to set config");

        jj_exec(
            &repo_path,
            ["config", "set", "--repo", "jj-vine.forge", "gitlab"],
        )
        .expect("Failed to set config");

        let config = Config::load(&repo_path).expect("Failed to load config");

        assert!(config.assign_to_self);
    }

    #[test]
    fn test_config_default_reviewers_empty() {
        let (_temp, repo_path) = create_test_repo();

        // Set required config only
        jj_exec(
            &repo_path,
            [
                "config",
                "set",
                "--repo",
                "jj-vine.gitlab.host",
                "https://gitlab.com",
            ],
        )
        .expect("Failed to set config");

        jj_exec(
            &repo_path,
            [
                "config",
                "set",
                "--repo",
                "jj-vine.gitlab.project",
                "test/proj",
            ],
        )
        .expect("Failed to set config");

        jj_exec(
            &repo_path,
            ["config", "set", "--repo", "jj-vine.gitlab.token", "token"],
        )
        .expect("Failed to set config");

        jj_exec(
            &repo_path,
            ["config", "set", "--repo", "jj-vine.forge", "gitlab"],
        )
        .expect("Failed to set config");

        let config = Config::load(&repo_path).expect("Failed to load config");

        assert!(config.default_reviewers.is_empty());
    }

    #[test]
    fn test_config_default_reviewers_single() {
        let (_temp, repo_path) = create_test_repo();

        // Set required config
        jj_exec(
            &repo_path,
            [
                "config",
                "set",
                "--repo",
                "jj-vine.gitlab.host",
                "https://gitlab.com",
            ],
        )
        .expect("Failed to set config");

        jj_exec(
            &repo_path,
            [
                "config",
                "set",
                "--repo",
                "jj-vine.gitlab.project",
                "test/proj",
            ],
        )
        .expect("Failed to set config");

        jj_exec(
            &repo_path,
            ["config", "set", "--repo", "jj-vine.gitlab.token", "token"],
        )
        .expect("Failed to set config");

        // Set single reviewer as TOML array
        jj_exec(
            &repo_path,
            [
                "config",
                "set",
                "--repo",
                "jj-vine.defaultReviewers",
                r#"["reviewer1"]"#,
            ],
        )
        .expect("Failed to set config");

        jj_exec(
            &repo_path,
            ["config", "set", "--repo", "jj-vine.forge", "gitlab"],
        )
        .expect("Failed to set config");

        let config = Config::load(&repo_path).expect("Failed to load config");

        assert_eq!(config.default_reviewers, vec!["reviewer1"]);
    }

    #[test]
    fn test_config_default_reviewers_multiple() {
        let (_temp, repo_path) = create_test_repo();

        // Set required config
        jj_exec(
            &repo_path,
            [
                "config",
                "set",
                "--repo",
                "jj-vine.gitlab.host",
                "https://gitlab.com",
            ],
        )
        .expect("Failed to set config");

        jj_exec(
            &repo_path,
            [
                "config",
                "set",
                "--repo",
                "jj-vine.gitlab.project",
                "test/proj",
            ],
        )
        .expect("Failed to set config");

        jj_exec(
            &repo_path,
            ["config", "set", "--repo", "jj-vine.gitlab.token", "token"],
        )
        .expect("Failed to set config");

        // Set multiple reviewers as TOML array
        jj_exec(
            &repo_path,
            [
                "config",
                "set",
                "--repo",
                "jj-vine.defaultReviewers",
                r#"["reviewer1", "reviewer2", "reviewer3"]"#,
            ],
        )
        .expect("Failed to set config");

        jj_exec(
            &repo_path,
            ["config", "set", "--repo", "jj-vine.forge", "gitlab"],
        )
        .expect("Failed to set config");

        let config = Config::load(&repo_path).expect("Failed to load config");

        assert_eq!(
            config.default_reviewers,
            vec!["reviewer1", "reviewer2", "reviewer3"]
        );
    }
}
