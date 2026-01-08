use crate::error::{Error, Result};
use crate::jj::run_jj_command;
use std::path::PathBuf;

/// Configuration for jj-mrs
///
/// Configuration is loaded from git config with priority:
/// CLI args > jj config > git config > defaults
#[derive(Debug, Clone)]
pub struct Config {
    /// GitLab instance URL (e.g., <https://gitlab.example.com>)
    pub gitlab_host: String,

    /// GitLab project ID (e.g., "group/project" or "12345")
    pub gitlab_project: String,

    /// GitLab Personal Access Token
    pub gitlab_token: String,

    /// Optional branch prefix (e.g., "mrs/")
    pub branch_prefix: Option<String>,

    /// Git remote name (default: "origin")
    pub remote_name: String,

    /// Default branch name (default: "main")
    pub default_branch: String,

    /// Optional path to CA bundle for TLS verification
    pub ca_bundle: Option<String>,

    /// Accept non-compliant TLS certificates (for certificates that don't meet strict X.509 standards)
    pub tls_accept_non_compliant_certs: bool,
}

impl Config {
    /// Load configuration from jj config
    ///
    /// Reads configuration keys:
    /// - spr.gitlabHost - GitLab instance URL
    /// - spr.gitlabProject - Project ID
    /// - spr.gitlabToken - Personal Access Token
    /// - spr.branchPrefix - Optional branch prefix
    /// - spr.remoteName - Git remote name
    /// - spr.defaultBranch - Default branch name
    /// - spr.caBundle - Optional path to CA bundle for TLS
    pub fn load(repo_path: &PathBuf) -> Result<Self> {
        // Helper to run jj config get
        let get_config = |key: &str| -> Result<Option<String>> {
            match run_jj_command(repo_path, &["config", "get", key]) {
                Ok(value) => {
                    let trimmed = value.trim();
                    // jj config get might return empty string or the literal string "null" for unset values
                    if trimmed.is_empty() || trimmed == "null" {
                        Ok(None)
                    } else {
                        // Remove surrounding quotes if present
                        let cleaned = trimmed.trim_matches('"').to_string();
                        Ok(Some(cleaned))
                    }
                }
                Err(Error::JjCommand { .. }) => Ok(None),
                Err(e) => Err(e),
            }
        };

        // Required fields
        let gitlab_host = get_config("spr.gitlabHost")?.ok_or_else(|| Error::Config {
            message: "Missing required config: spr.gitlabHost".to_string(),
        })?;

        let gitlab_project = get_config("spr.gitlabProject")?.ok_or_else(|| Error::Config {
            message: "Missing required config: spr.gitlabProject".to_string(),
        })?;

        let gitlab_token = get_config("spr.gitlabToken")?.ok_or_else(|| Error::Config {
            message: "Missing required config: spr.gitlabToken".to_string(),
        })?;

        // Optional fields with defaults
        let branch_prefix = get_config("spr.branchPrefix")?;
        let remote_name = get_config("spr.remoteName")?.unwrap_or_else(|| "origin".to_string());
        let default_branch = get_config("spr.defaultBranch")?.unwrap_or_else(|| "main".to_string());
        let ca_bundle = get_config("spr.caBundle")?;
        let tls_accept_non_compliant_certs = get_config("spr.tlsAcceptNonCompliantCerts")?
            .map(|v| v == "true" || v == "1" || v == "yes")
            .unwrap_or(false);

        Ok(Config {
            gitlab_host,
            gitlab_project,
            gitlab_token,
            branch_prefix,
            remote_name,
            default_branch,
            ca_bundle,
            tls_accept_non_compliant_certs,
        })
    }

    /// Check if all required configuration is present
    pub fn validate(&self) -> Result<()> {
        if self.gitlab_host.is_empty() {
            return Err(Error::Config {
                message: "gitlab_host cannot be empty".to_string(),
            });
        }

        if self.gitlab_project.is_empty() {
            return Err(Error::Config {
                message: "gitlab_project cannot be empty".to_string(),
            });
        }

        if self.gitlab_token.is_empty() {
            return Err(Error::Config {
                message: "gitlab_token cannot be empty".to_string(),
            });
        }

        Ok(())
    }

    /// Apply branch prefix to a bookmark name if configured
    pub fn apply_branch_prefix(&self, bookmark: &str) -> String {
        match &self.branch_prefix {
            Some(prefix) => format!("{}{}", prefix, bookmark),
            None => bookmark.to_string(),
        }
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
            assert!(message.contains("spr.gitlabHost"));
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
                "spr.gitlabHost",
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
                "spr.gitlabProject",
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
                "spr.gitlabToken",
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
        assert!(config.branch_prefix.is_none());
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
                "spr.gitlabHost",
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
                "spr.gitlabProject",
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
                "spr.gitlabToken",
                "glpat-test123",
            ],
        )
        .expect("Failed to set config");

        run_jj_command(
            &repo_path,
            &["config", "set", "--repo", "spr.branchPrefix", "mrs/"],
        )
        .expect("Failed to set config");

        run_jj_command(
            &repo_path,
            &["config", "set", "--repo", "spr.remoteName", "upstream"],
        )
        .expect("Failed to set config");

        run_jj_command(
            &repo_path,
            &["config", "set", "--repo", "spr.defaultBranch", "master"],
        )
        .expect("Failed to set config");

        // Load config
        let config = Config::load(&repo_path).expect("Failed to load config");

        assert_eq!(config.gitlab_host, "https://gitlab.example.com");
        assert_eq!(config.gitlab_project, "my-group/my-project");
        assert_eq!(config.gitlab_token, "glpat-test123");
        assert_eq!(config.branch_prefix, Some("mrs/".to_string()));
        assert_eq!(config.remote_name, "upstream");
        assert_eq!(config.default_branch, "master");
    }

    #[test]
    fn test_validate() {
        let config = Config {
            gitlab_host: "https://gitlab.example.com".to_string(),
            gitlab_project: "group/project".to_string(),
            gitlab_token: "token".to_string(),
            branch_prefix: None,
            remote_name: "origin".to_string(),
            default_branch: "main".to_string(),
            ca_bundle: None,
            tls_accept_non_compliant_certs: false,
        };

        assert!(config.validate().is_ok());

        let invalid_config = Config {
            gitlab_host: "".to_string(),
            gitlab_project: "group/project".to_string(),
            gitlab_token: "token".to_string(),
            branch_prefix: None,
            remote_name: "origin".to_string(),
            default_branch: "main".to_string(),
            ca_bundle: None,
            tls_accept_non_compliant_certs: false,
        };

        assert!(invalid_config.validate().is_err());
    }

    #[test]
    fn test_apply_branch_prefix() {
        let config_with_prefix = Config {
            gitlab_host: "https://gitlab.example.com".to_string(),
            gitlab_project: "group/project".to_string(),
            gitlab_token: "token".to_string(),
            branch_prefix: Some("mrs/".to_string()),
            remote_name: "origin".to_string(),
            default_branch: "main".to_string(),
            ca_bundle: None,
            tls_accept_non_compliant_certs: false,
        };

        assert_eq!(
            config_with_prefix.apply_branch_prefix("feature"),
            "mrs/feature"
        );

        let config_without_prefix = Config {
            gitlab_host: "https://gitlab.example.com".to_string(),
            gitlab_project: "group/project".to_string(),
            gitlab_token: "token".to_string(),
            branch_prefix: None,
            remote_name: "origin".to_string(),
            default_branch: "main".to_string(),
            ca_bundle: None,
            tls_accept_non_compliant_certs: false,
        };

        assert_eq!(
            config_without_prefix.apply_branch_prefix("feature"),
            "feature"
        );
    }
}
