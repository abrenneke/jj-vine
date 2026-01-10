use crate::error::{Error, Result};
use crate::jj::run_jj_command;
use serde::Deserialize;
use std::path::PathBuf;

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

/// Configuration for jj-mrs
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    /// GitLab instance URL (e.g., <https://gitlab.example.com>)
    pub gitlab_host: String,

    /// GitLab project ID (e.g., "group/project" or "12345")
    pub gitlab_project: String,

    /// GitLab Personal Access Token
    pub gitlab_token: String,

    /// Git remote name (default: "origin")
    #[serde(default = "default_remote_name")]
    pub remote_name: String,

    /// Default branch name (default: "main")
    #[serde(default = "default_branch")]
    pub default_branch: String,

    /// Optional path to CA bundle for TLS verification
    #[serde(default)]
    pub ca_bundle: Option<String>,

    /// Accept non-compliant TLS certificates (for certificates that don't meet strict X.509 standards)
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
}

impl Config {
    /// Load configuration from jj config
    pub fn load(repo_path: &PathBuf) -> Result<Self> {
        let output = run_jj_command(repo_path, &["config", "list"])?;

        let toml_value: toml::Value =
            toml::from_str(&output.stdout).map_err(|e| Error::Config {
                message: format!("Failed to parse config as TOML: {}", e),
            })?;

        let jj_mrs_value = toml_value.get("jj-mrs").ok_or_else(|| Error::Config {
            message: "Missing required config section: jj-mrs".to_string(),
        })?;

        let config: Config = jj_mrs_value.clone().try_into().map_err(|e| Error::Config {
            message: format!("Failed to parse jj-mrs config: {}", e),
        })?;

        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_repo() -> (TempDir, PathBuf) {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let repo_path = temp_dir.path().to_path_buf();

        // Initialize jj repo
        run_jj_command(&repo_path, &["git", "init", "--colocate"]).expect("Failed to init jj repo");

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
                    || message.contains("jj-mrs"),
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
        run_jj_command(
            &repo_path,
            &[
                "config",
                "set",
                "--repo",
                "jj-mrs.gitlabHost",
                "https://gitlab.example.com",
            ],
        )
        .expect("Failed to set config");

        run_jj_command(
            &repo_path,
            &[
                "config",
                "set",
                "--repo",
                "jj-mrs.gitlabProject",
                "my-group/my-project",
            ],
        )
        .expect("Failed to set config");

        run_jj_command(
            &repo_path,
            &[
                "config",
                "set",
                "--repo",
                "jj-mrs.gitlabToken",
                "glpat-test123",
            ],
        )
        .expect("Failed to set config");

        // Load config
        let config = Config::load(&repo_path).expect("Failed to load config");

        assert_eq!(config.gitlab_host, "https://gitlab.example.com");
        assert_eq!(config.gitlab_project, "my-group/my-project");
        assert_eq!(config.gitlab_token, "glpat-test123");
        assert_eq!(config.remote_name, "origin");
        assert_eq!(config.default_branch, "main");
    }

    #[test]
    fn test_config_with_optional_fields() {
        let (_temp, repo_path) = create_test_repo();

        // Set all config including optional fields
        run_jj_command(
            &repo_path,
            &[
                "config",
                "set",
                "--repo",
                "jj-mrs.gitlabHost",
                "https://gitlab.example.com",
            ],
        )
        .expect("Failed to set config");

        run_jj_command(
            &repo_path,
            &[
                "config",
                "set",
                "--repo",
                "jj-mrs.gitlabProject",
                "my-group/my-project",
            ],
        )
        .expect("Failed to set config");

        run_jj_command(
            &repo_path,
            &[
                "config",
                "set",
                "--repo",
                "jj-mrs.gitlabToken",
                "glpat-test123",
            ],
        )
        .expect("Failed to set config");

        run_jj_command(
            &repo_path,
            &["config", "set", "--repo", "jj-mrs.branchPrefix", "mrs/"],
        )
        .expect("Failed to set config");

        run_jj_command(
            &repo_path,
            &["config", "set", "--repo", "jj-mrs.remoteName", "upstream"],
        )
        .expect("Failed to set config");

        run_jj_command(
            &repo_path,
            &["config", "set", "--repo", "jj-mrs.defaultBranch", "master"],
        )
        .expect("Failed to set config");

        // Load config
        let config = Config::load(&repo_path).expect("Failed to load config");

        assert_eq!(config.gitlab_host, "https://gitlab.example.com");
        assert_eq!(config.gitlab_project, "my-group/my-project");
        assert_eq!(config.gitlab_token, "glpat-test123");
        assert_eq!(config.remote_name, "upstream");
        assert_eq!(config.default_branch, "master");
    }

    #[test]
    fn test_config_default_stack_visualization() {
        let (_temp, repo_path) = create_test_repo();

        // Set required config, but not stack visualization config
        run_jj_command(
            &repo_path,
            &[
                "config",
                "set",
                "--repo",
                "jj-mrs.gitlabHost",
                "https://gitlab.com",
            ],
        )
        .expect("Failed to set config");

        run_jj_command(
            &repo_path,
            &[
                "config",
                "set",
                "--repo",
                "jj-mrs.gitlabProject",
                "test/proj",
            ],
        )
        .expect("Failed to set config");

        run_jj_command(
            &repo_path,
            &["config", "set", "--repo", "jj-mrs.gitlabToken", "token"],
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
        run_jj_command(
            &repo_path,
            &[
                "config",
                "set",
                "--repo",
                "jj-mrs.gitlabHost",
                "https://gitlab.com",
            ],
        )
        .expect("Failed to set config");

        run_jj_command(
            &repo_path,
            &[
                "config",
                "set",
                "--repo",
                "jj-mrs.gitlabProject",
                "test/proj",
            ],
        )
        .expect("Failed to set config");

        run_jj_command(
            &repo_path,
            &["config", "set", "--repo", "jj-mrs.gitlabToken", "token"],
        )
        .expect("Failed to set config");

        // Set explicit stack visualization config (disable it)
        run_jj_command(
            &repo_path,
            &[
                "config",
                "set",
                "--repo",
                "jj-mrs.enableStackVisualization",
                "false",
            ],
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
        run_jj_command(
            &repo_path,
            &[
                "config",
                "set",
                "--repo",
                "jj-mrs.gitlabHost",
                "https://gitlab.com",
            ],
        )
        .expect("Failed to set config");

        run_jj_command(
            &repo_path,
            &[
                "config",
                "set",
                "--repo",
                "jj-mrs.gitlabProject",
                "test/proj",
            ],
        )
        .expect("Failed to set config");

        run_jj_command(
            &repo_path,
            &["config", "set", "--repo", "jj-mrs.gitlabToken", "token"],
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
        run_jj_command(
            &repo_path,
            &[
                "config",
                "set",
                "--repo",
                "jj-mrs.gitlabHost",
                "https://gitlab.com",
            ],
        )
        .expect("Failed to set config");

        run_jj_command(
            &repo_path,
            &[
                "config",
                "set",
                "--repo",
                "jj-mrs.gitlabProject",
                "test/proj",
            ],
        )
        .expect("Failed to set config");

        run_jj_command(
            &repo_path,
            &["config", "set", "--repo", "jj-mrs.gitlabToken", "token"],
        )
        .expect("Failed to set config");

        // Set explicit MR settings (opposite of defaults)
        run_jj_command(
            &repo_path,
            &[
                "config",
                "set",
                "--repo",
                "jj-mrs.deleteSourceBranch",
                "false",
            ],
        )
        .expect("Failed to set config");

        run_jj_command(
            &repo_path,
            &["config", "set", "--repo", "jj-mrs.squashCommits", "true"],
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
        run_jj_command(
            &repo_path,
            &[
                "config",
                "set",
                "--repo",
                "jj-mrs.gitlabHost",
                "https://gitlab.com",
            ],
        )
        .expect("Failed to set config");

        run_jj_command(
            &repo_path,
            &[
                "config",
                "set",
                "--repo",
                "jj-mrs.gitlabProject",
                "test/proj",
            ],
        )
        .expect("Failed to set config");

        run_jj_command(
            &repo_path,
            &["config", "set", "--repo", "jj-mrs.gitlabToken", "token"],
        )
        .expect("Failed to set config");

        let config = Config::load(&repo_path).expect("Failed to load config");

        assert!(!config.assign_to_self);
    }

    #[test]
    fn test_config_explicit_assign_to_self() {
        let (_temp, repo_path) = create_test_repo();

        // Set required config
        run_jj_command(
            &repo_path,
            &[
                "config",
                "set",
                "--repo",
                "jj-mrs.gitlabHost",
                "https://gitlab.com",
            ],
        )
        .expect("Failed to set config");

        run_jj_command(
            &repo_path,
            &[
                "config",
                "set",
                "--repo",
                "jj-mrs.gitlabProject",
                "test/proj",
            ],
        )
        .expect("Failed to set config");

        run_jj_command(
            &repo_path,
            &["config", "set", "--repo", "jj-mrs.gitlabToken", "token"],
        )
        .expect("Failed to set config");

        // Set assign_to_self to true
        run_jj_command(
            &repo_path,
            &["config", "set", "--repo", "jj-mrs.assignToSelf", "true"],
        )
        .expect("Failed to set config");

        let config = Config::load(&repo_path).expect("Failed to load config");

        assert!(config.assign_to_self);
    }

    #[test]
    fn test_config_default_reviewers_empty() {
        let (_temp, repo_path) = create_test_repo();

        // Set required config only
        run_jj_command(
            &repo_path,
            &[
                "config",
                "set",
                "--repo",
                "jj-mrs.gitlabHost",
                "https://gitlab.com",
            ],
        )
        .expect("Failed to set config");

        run_jj_command(
            &repo_path,
            &[
                "config",
                "set",
                "--repo",
                "jj-mrs.gitlabProject",
                "test/proj",
            ],
        )
        .expect("Failed to set config");

        run_jj_command(
            &repo_path,
            &["config", "set", "--repo", "jj-mrs.gitlabToken", "token"],
        )
        .expect("Failed to set config");

        let config = Config::load(&repo_path).expect("Failed to load config");

        assert!(config.default_reviewers.is_empty());
    }

    #[test]
    fn test_config_default_reviewers_single() {
        let (_temp, repo_path) = create_test_repo();

        // Set required config
        run_jj_command(
            &repo_path,
            &[
                "config",
                "set",
                "--repo",
                "jj-mrs.gitlabHost",
                "https://gitlab.com",
            ],
        )
        .expect("Failed to set config");

        run_jj_command(
            &repo_path,
            &[
                "config",
                "set",
                "--repo",
                "jj-mrs.gitlabProject",
                "test/proj",
            ],
        )
        .expect("Failed to set config");

        run_jj_command(
            &repo_path,
            &["config", "set", "--repo", "jj-mrs.gitlabToken", "token"],
        )
        .expect("Failed to set config");

        // Set single reviewer as TOML array
        run_jj_command(
            &repo_path,
            &[
                "config",
                "set",
                "--repo",
                "jj-mrs.defaultReviewers",
                r#"["reviewer1"]"#,
            ],
        )
        .expect("Failed to set config");

        let config = Config::load(&repo_path).expect("Failed to load config");

        assert_eq!(config.default_reviewers, vec!["reviewer1"]);
    }

    #[test]
    fn test_config_default_reviewers_multiple() {
        let (_temp, repo_path) = create_test_repo();

        // Set required config
        run_jj_command(
            &repo_path,
            &[
                "config",
                "set",
                "--repo",
                "jj-mrs.gitlabHost",
                "https://gitlab.com",
            ],
        )
        .expect("Failed to set config");

        run_jj_command(
            &repo_path,
            &[
                "config",
                "set",
                "--repo",
                "jj-mrs.gitlabProject",
                "test/proj",
            ],
        )
        .expect("Failed to set config");

        run_jj_command(
            &repo_path,
            &["config", "set", "--repo", "jj-mrs.gitlabToken", "token"],
        )
        .expect("Failed to set config");

        // Set multiple reviewers as TOML array
        run_jj_command(
            &repo_path,
            &[
                "config",
                "set",
                "--repo",
                "jj-mrs.defaultReviewers",
                r#"["reviewer1", "reviewer2", "reviewer3"]"#,
            ],
        )
        .expect("Failed to set config");

        let config = Config::load(&repo_path).expect("Failed to load config");

        assert_eq!(
            config.default_reviewers,
            vec!["reviewer1", "reviewer2", "reviewer3"]
        );
    }
}
