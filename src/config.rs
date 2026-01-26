use std::path::PathBuf;

use bon::Builder;
use serde::Deserialize;

use crate::{
    error::{ConfigSnafu, Error, Result},
    jj::Jujutsu,
};

/// Forge type (GitLab, GitHub, or Forgejo)
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, strum::VariantArray)]
#[serde(rename_all = "lowercase")]
pub enum ForgeType {
    /// GitLab (GitLab.com or self-hosted)
    GitLab,

    /// GitHub (GitHub.com or GitHub Enterprise)
    GitHub,

    /// Forgejo/Gitea (self-hosted or Codeberg)
    Forgejo,

    /// Azure DevOps
    #[serde(rename = "azure")]
    AzureDevOps,
}

impl ForgeType {
    pub fn display_name(&self) -> &str {
        match self {
            ForgeType::GitLab => "GitLab",
            ForgeType::GitHub => "GitHub",
            ForgeType::Forgejo => "Forgejo",
            ForgeType::AzureDevOps => "Azure DevOps",
        }
    }
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
                ForgeType::AzureDevOps => "azure",
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
            "azure" => Ok(Self::AzureDevOps),
            _ => Err(ConfigSnafu {
                message: format!("Invalid forge type: {}", s),
            }
            .build()),
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
        } else if host.contains("azure") {
            Some(Self::AzureDevOps)
        } else {
            None
        }
    }
}

/// Stack visualization format
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DescriptionFormat {
    /// Do not render this stack visualization
    None,

    /// A linear numbered list
    #[default]
    Linear,

    /// A tree of MRs where children are indented
    Tree,
}

fn default_remote_name() -> String {
    "origin".to_string()
}

const fn default_true() -> bool {
    true
}

/// Configuration for jj-vine
#[derive(Debug, Clone, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    /// Which forge to use.
    pub forge: ForgeType,

    /// The branch name to use for MRs into `trunk()`. You will generally only
    /// need to set this explicitly if you use a branch name other than
    /// `main`, `master`, or `trunk`, and jj-vine is having difficulty
    /// detecting the correct branch name automatically.
    #[serde(default)]
    pub default_base_branch: Option<String>,

    // ===== Common Configuration =====
    /// Git remote name (defaults to "origin").
    #[serde(default = "default_remote_name")]
    #[builder(default = default_remote_name())]
    pub remote_name: String,

    /// Optional path to CA bundle for TLS verification. Only useful if you
    /// have a self-hosted forge without a publicly trusted certificate.
    #[serde(default)]
    pub ca_bundle: Option<String>,

    /// Accept non-compliant TLS certificates (for certificates that don't meet
    /// strict X.509 standards). This is almost always unnecessary unless
    /// you have a unique situation.
    #[serde(default)]
    #[builder(default)]
    pub tls_accept_non_compliant_certs: bool,

    /// Configuration for MR description generation.
    #[serde(default)]
    #[builder(default)]
    pub description: DescriptionConfig,

    /// Delete source branch when MR is merged (defaults to true).
    ///
    /// Unsupported by GitHub and Forgejo - this option will have no effect.
    /// Those forges only offer this as a repository-level default +
    /// on-merge flag.
    #[serde(default = "default_true")]
    #[builder(default)]
    pub delete_source_branch: bool,

    /// Squash commits when MR is merged (defaults to false).
    ///
    /// Unsupported by GitHub and Forgejo - this option will have no effect.
    /// Those forges only offer this as a repository-level default +
    /// on-merge flag.
    #[serde(default)]
    #[builder(default)]
    pub squash_commits: bool,

    /// Assign created MRs to yourself (defaults to false).
    #[serde(default)]
    #[builder(default)]
    pub assign_to_self: bool,

    /// Default reviewers for created MRs (list of usernames).
    #[serde(default)]
    #[builder(default)]
    pub default_reviewers: Vec<String>,

    /// Open newly created MRs as drafts (defaults to false).
    ///
    /// On Forgejo and GitLab, this will add "WIP: " and "Draft: " prefixes to
    /// the MR titles, respectively. This is configurable for Forgejo using the
    /// `jj-vine.forgejo.wip_prefix` setting.
    #[serde(default)]
    #[builder(default)]
    pub open_as_draft: bool,

    /// GitLab configuration.
    #[serde(default)]
    #[builder(default)]
    pub gitlab: GitLabConfig,

    /// GitHub configuration.
    #[serde(default)]
    #[builder(default)]
    pub github: GitHubConfig,

    /// Forgejo/Gitea/Codeberg configuration.
    #[serde(default)]
    #[builder(default)]
    pub forgejo: ForgejoConfig,

    /// Azure DevOps configuration.
    #[serde(default)]
    #[builder(default)]
    pub azure: AzureDevOpsConfig,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GitLabConfig {
    /// GitLab instance URL (e.g., `https://gitlab.example.com`).
    #[serde(default)]
    pub host: String,

    /// GitLab project ID (e.g., `group/project` or `12345`).
    #[serde(default)]
    pub project: String,

    /// Target project for MRs (if different from project, enables fork
    /// workflow).
    #[serde(default)]
    pub target_project: String,

    /// GitLab Personal Access Token.
    #[serde(default)]
    pub token: String,

    /// If true, jj-vine will create dependencies between merge requests,
    /// requiring that all parent merge requests are merged before the child
    /// merge request can be merged.
    #[serde(default = "default_true")]
    pub create_merge_request_dependencies: bool,
}

impl GitLabConfig {
    /// Get the project where MRs target.
    pub fn target_project(&self) -> &str {
        if self.target_project.is_empty() {
            &self.project
        } else {
            &self.target_project
        }
    }

    /// Get the project where branches are pushed.
    pub fn source_project(&self) -> &str {
        &self.project
    }

    /// Check if this is a fork workflow (target differs from source).
    pub fn is_fork_workflow(&self) -> bool {
        self.target_project() != self.project
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GitHubConfig {
    /// GitHub API URL (e.g., `https://api.github.com` or `https://github.example.com/api/v3`).
    #[serde(default)]
    pub host: String,

    /// GitHub repository in `owner/repo` format.
    #[serde(default)]
    pub project: String,

    /// Target repository for PRs (if different from project, enables fork
    /// workflow).
    #[serde(default)]
    pub target_project: String,

    /// GitHub Personal Access Token.
    #[serde(default)]
    pub token: String,
}

impl GitHubConfig {
    /// Get the repository where PRs target.
    pub fn target_project(&self) -> &str {
        if self.target_project.is_empty() {
            &self.project
        } else {
            &self.target_project
        }
    }

    /// Get the repository where branches are pushed.
    pub fn source_project(&self) -> &str {
        &self.project
    }

    /// Check if this is a fork workflow (target differs from source).
    pub fn is_fork_workflow(&self) -> bool {
        self.target_project() != self.project
    }
}

/// Not exactly documented, but the default repository setting for Forgejo is:
/// ```go
/// WorkInProgressPrefixes: []string{"WIP:", "[WIP]"}
/// ```
fn default_wip_prefix() -> String {
    "WIP: ".to_string()
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ForgejoConfig {
    /// Forgejo/Gitea instance URL (e.g., `https://codeberg.org`).
    #[serde(default)]
    pub host: String,

    /// Repository in `owner/repo` format.
    #[serde(default)]
    pub project: String,

    /// Target repository for PRs (if different from project, enables fork
    /// workflow).
    #[serde(default)]
    pub target_project: String,

    /// API access token.
    #[serde(default)]
    pub token: String,

    /// Prefix for WIP merge requests.
    #[serde(default = "default_wip_prefix")]
    pub wip_prefix: String,
}

impl ForgejoConfig {
    /// Get the repository where PRs target.
    pub fn target_project(&self) -> &str {
        if self.target_project.is_empty() {
            &self.project
        } else {
            &self.target_project
        }
    }

    /// Get the repository where branches are pushed.
    pub fn source_project(&self) -> &str {
        &self.project
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AzureDevOpsConfig {
    /// Azure DevOps instance URL (e.g., `https://dev.azure.com`).
    #[serde(default)]
    pub host: String,

    #[serde(default)]
    pub project: String,

    #[serde(default)]
    pub target_project: String,

    #[serde(default)]
    pub token: String,

    #[serde(default)]
    pub source_repository_name: Option<String>,

    #[serde(default)]
    pub target_repository_name: Option<String>,

    #[serde(default)]
    pub source_repository_id: Option<String>,

    #[serde(default)]
    pub target_repository_id: Option<String>,

    #[serde(default)]
    pub vssps_host: String,
}

impl AzureDevOpsConfig {
    pub fn source_project_id(&self) -> &str {
        &self.project
    }

    pub fn target_project_id(&self) -> &str {
        if self.target_project.is_empty() {
            &self.project
        } else {
            &self.target_project
        }
    }

    pub fn target_repository_name(&self) -> Option<&str> {
        if self.target_repository_name.is_none() {
            self.source_repository_name.as_deref()
        } else {
            self.target_repository_name.as_deref()
        }
    }

    pub fn target_repository_id(&self) -> Option<&str> {
        if self.target_repository_id.is_none() {
            self.source_repository_id.as_deref()
        } else {
            self.target_repository_id.as_deref()
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DescriptionConfig {
    /// Whether to enable or disable description generation entirely.
    pub enabled: bool,

    /// How to render the description for different types of merge request
    /// stacks.
    #[serde(default)]
    pub format: DescriptionFormatsConfig,
}

impl Default for DescriptionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            format: Default::default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DescriptionFormatsConfig {
    /// How to render a single merge request, without any parents or children
    /// besides the trunk. Defaults to not rendering a description.
    pub single: DescriptionFormat,

    /// How to render a linear stack of MRs.
    /// Defaults to a linear numbered list.
    pub linear: DescriptionFormat,

    /// How to render a tree of MRs, where two MRs merge into a common parent.
    /// Defaults to a linear numbered list.
    pub tree: DescriptionFormat,

    /// How to render a complex graph of MRs, where two MRs merge into a common
    /// parent, or any merge request has multiple parents.
    /// Defaults to a linear numbered list.
    pub complex: DescriptionFormat,
}

impl Default for DescriptionFormatsConfig {
    fn default() -> Self {
        Self {
            single: DescriptionFormat::None,
            linear: DescriptionFormat::Linear,
            tree: DescriptionFormat::Linear,
            complex: DescriptionFormat::Linear,
        }
    }
}

impl Config {
    /// Load configuration from jj config
    pub fn load(repo_path: impl Into<PathBuf>) -> Result<Self> {
        let jj = Jujutsu::new(repo_path)?;
        let output = jj.exec(["config", "list"])?;

        let toml_value: toml::Value = toml::from_str(&output.stdout).map_err(|e| {
            ConfigSnafu {
                message: format!("Failed to parse config as TOML: {}", e),
            }
            .build()
        })?;

        let jj_vine_value = toml_value.get("jj-vine").ok_or_else(|| {
            ConfigSnafu {
                message: "Missing required config section: jj-vine".to_string(),
            }
            .build()
        })?;

        let config: Config = jj_vine_value.clone().try_into().map_err(|e| {
            ConfigSnafu {
                message: format!("Failed to parse jj-vine config: {}", e),
            }
            .build()
        })?;

        config.validate()?;

        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        match self.forge {
            ForgeType::GitLab => {
                if self.gitlab.host.is_empty() {
                    return Err(ConfigSnafu {
                        message: "gitlab.host is required when forge is gitlab".to_string(),
                    }
                    .build());
                }
                if self.gitlab.project.is_empty() {
                    return Err(ConfigSnafu {
                        message: "gitlab.project is required when forge is gitlab".to_string(),
                    }
                    .build());
                }
                if self.gitlab.token.is_empty() {
                    return Err(ConfigSnafu {
                        message: "gitlab.token is required when forge is gitlab".to_string(),
                    }
                    .build());
                }
            }
            ForgeType::GitHub => {
                if self.github.project.is_empty() {
                    return Err(ConfigSnafu {
                        message: "github.project is required when forge is github".to_string(),
                    }
                    .build());
                }
                if self.github.token.is_empty() {
                    return Err(ConfigSnafu {
                        message: "github.token is required when forge is github".to_string(),
                    }
                    .build());
                }
            }
            ForgeType::Forgejo => {
                if self.forgejo.host.is_empty() {
                    return Err(ConfigSnafu {
                        message: "forgejo.host is required when forge is forgejo".to_string(),
                    }
                    .build());
                }
                if self.forgejo.project.is_empty() {
                    return Err(ConfigSnafu {
                        message: "forgejo.project is required when forge is forgejo".to_string(),
                    }
                    .build());
                }
                if self.forgejo.token.is_empty() {
                    return Err(ConfigSnafu {
                        message: "forgejo.token is required when forge is forgejo".to_string(),
                    }
                    .build());
                }
            }
            ForgeType::AzureDevOps => {
                if self.azure.host.is_empty() {
                    return ConfigSnafu {
                        message: "azure.host is required when forge is azure".to_string(),
                    }
                    .fail();
                }
                if self.azure.project.is_empty() {
                    return ConfigSnafu {
                        message: "azure.project is required when forge is azure".to_string(),
                    }
                    .fail();
                }
                if self.azure.token.is_empty() {
                    return ConfigSnafu {
                        message: "azure.token is required when forge is azure".to_string(),
                    }
                    .fail();
                }
                if self.azure.source_repository_name.is_none()
                    && self.azure.source_repository_id.is_none()
                {
                    return ConfigSnafu {
                        message: "azure.source_repository_name or azure.source_repository_id is required when forge is azure".to_string(),
                    }
                    .fail();
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

        let jj = Jujutsu::new(&repo_path).expect("Failed to create Jujutsu instance");
        jj.exec(["git", "init", "--colocate"])
            .expect("Failed to init jj repo");

        (temp_dir, repo_path)
    }

    #[test]
    fn test_config_load_missing_required() {
        let (_temp, repo_path) = create_test_repo();

        // Try to load config without setting anything
        let result = Config::load(&repo_path);
        assert!(result.is_err());

        if let Err(Error::Config { message, .. }) = result {
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

        let jj = Jujutsu::new(&repo_path).expect("Failed to create Jujutsu instance");
        // Set required config
        jj.exec([
            "config",
            "set",
            "--repo",
            "jj-vine.gitlab.host",
            "https://gitlab.example.com",
        ])
        .expect("Failed to set config");

        jj.exec([
            "config",
            "set",
            "--repo",
            "jj-vine.gitlab.project",
            "my-group/my-project",
        ])
        .expect("Failed to set config");

        jj.exec([
            "config",
            "set",
            "--repo",
            "jj-vine.gitlab.token",
            "glpat-test123",
        ])
        .expect("Failed to set config");

        jj.exec(["config", "set", "--repo", "jj-vine.forge", "gitlab"])
            .expect("Failed to set config");

        // Load config
        let config = Config::load(&repo_path).expect("Failed to load config");

        assert_eq!(config.gitlab.host, "https://gitlab.example.com".to_string());
        assert_eq!(config.gitlab.project, "my-group/my-project".to_string());
        assert_eq!(config.gitlab.token, "glpat-test123".to_string());
        assert_eq!(config.remote_name, "origin");
    }

    #[test]
    fn test_config_with_optional_fields() {
        let (_temp, repo_path) = create_test_repo();

        let jj = Jujutsu::new(&repo_path).expect("Failed to create Jujutsu instance");
        // Set all config including optional fields
        jj.exec([
            "config",
            "set",
            "--repo",
            "jj-vine.gitlab.host",
            "https://gitlab.example.com",
        ])
        .expect("Failed to set config");

        jj.exec([
            "config",
            "set",
            "--repo",
            "jj-vine.gitlab.project",
            "my-group/my-project",
        ])
        .expect("Failed to set config");

        jj.exec([
            "config",
            "set",
            "--repo",
            "jj-vine.gitlab.token",
            "glpat-test123",
        ])
        .expect("Failed to set config");

        jj.exec(["config", "set", "--repo", "jj-vine.branchPrefix", "mrs/"])
            .expect("Failed to set config");

        jj.exec(["config", "set", "--repo", "jj-vine.remoteName", "upstream"])
            .expect("Failed to set config");

        jj.exec(["config", "set", "--repo", "jj-vine.defaultBranch", "master"])
            .expect("Failed to set config");

        jj.exec(["config", "set", "--repo", "jj-vine.forge", "gitlab"])
            .expect("Failed to set config");

        // Load config
        let config = Config::load(&repo_path).expect("Failed to load config");

        assert_eq!(config.gitlab.host, "https://gitlab.example.com".to_string());
        assert_eq!(config.gitlab.project, "my-group/my-project".to_string());
        assert_eq!(config.gitlab.token, "glpat-test123".to_string());
        assert_eq!(config.remote_name, "upstream");
    }

    #[test]
    fn test_config_default_stack_visualization() {
        let (_temp, repo_path) = create_test_repo();

        let jj = Jujutsu::new(&repo_path).expect("Failed to create Jujutsu instance");
        // Set required config, but not stack visualization config
        jj.exec([
            "config",
            "set",
            "--repo",
            "jj-vine.gitlab.host",
            "https://gitlab.com",
        ])
        .expect("Failed to set config");

        jj.exec([
            "config",
            "set",
            "--repo",
            "jj-vine.gitlab.project",
            "test/proj",
        ])
        .expect("Failed to set config");

        jj.exec(["config", "set", "--repo", "jj-vine.gitlab.token", "token"])
            .expect("Failed to set config");

        jj.exec(["config", "set", "--repo", "jj-vine.forge", "gitlab"])
            .expect("Failed to set config");

        let config = Config::load(&repo_path).expect("Failed to load config");

        assert!(config.description.enabled);
        assert!(matches!(
            config.description.format.single,
            DescriptionFormat::None
        ));
        assert!(matches!(
            config.description.format.linear,
            DescriptionFormat::Linear
        ));
        assert!(matches!(
            config.description.format.tree,
            DescriptionFormat::Linear
        ));
        assert!(matches!(
            config.description.format.complex,
            DescriptionFormat::Linear
        ));
    }

    #[test]
    fn test_config_explicit_stack_visualization() {
        let (_temp, repo_path) = create_test_repo();

        let jj = Jujutsu::new(&repo_path).expect("Failed to create Jujutsu instance");
        // Set required config
        jj.exec([
            "config",
            "set",
            "--repo",
            "jj-vine.gitlab.host",
            "https://gitlab.com",
        ])
        .expect("Failed to set config");

        jj.exec([
            "config",
            "set",
            "--repo",
            "jj-vine.gitlab.project",
            "test/proj",
        ])
        .expect("Failed to set config");

        jj.exec(["config", "set", "--repo", "jj-vine.gitlab.token", "token"])
            .expect("Failed to set config");

        jj.exec([
            "config",
            "set",
            "--repo",
            "jj-vine.description.enabled",
            "false",
        ])
        .expect("Failed to set config");

        jj.exec(["config", "set", "--repo", "jj-vine.forge", "gitlab"])
            .expect("Failed to set config");

        let config = Config::load(&repo_path).expect("Failed to load config");

        assert!(!config.description.enabled);
        assert!(matches!(
            config.description.format.single,
            DescriptionFormat::None
        ));
        assert!(matches!(
            config.description.format.linear,
            DescriptionFormat::Linear
        ));
        assert!(matches!(
            config.description.format.tree,
            DescriptionFormat::Linear
        ));
        assert!(matches!(
            config.description.format.complex,
            DescriptionFormat::Linear
        ));
    }

    #[test]
    fn test_config_default_mr_settings() {
        let (_temp, repo_path) = create_test_repo();

        let jj = Jujutsu::new(&repo_path).expect("Failed to create Jujutsu instance");
        // Set required config only, don't set MR settings
        jj.exec([
            "config",
            "set",
            "--repo",
            "jj-vine.gitlab.host",
            "https://gitlab.com",
        ])
        .expect("Failed to set config");

        jj.exec([
            "config",
            "set",
            "--repo",
            "jj-vine.gitlab.project",
            "test/proj",
        ])
        .expect("Failed to set config");

        jj.exec(["config", "set", "--repo", "jj-vine.gitlab.token", "token"])
            .expect("Failed to set config");

        jj.exec(["config", "set", "--repo", "jj-vine.forge", "gitlab"])
            .expect("Failed to set config");

        let config = Config::load(&repo_path).expect("Failed to load config");

        assert!(config.delete_source_branch);
        assert!(!config.squash_commits);
    }

    #[test]
    fn test_config_explicit_mr_settings() {
        let (_temp, repo_path) = create_test_repo();

        let jj = Jujutsu::new(&repo_path).expect("Failed to create Jujutsu instance");
        // Set required config
        jj.exec([
            "config",
            "set",
            "--repo",
            "jj-vine.gitlab.host",
            "https://gitlab.com",
        ])
        .expect("Failed to set config");

        jj.exec([
            "config",
            "set",
            "--repo",
            "jj-vine.gitlab.project",
            "test/proj",
        ])
        .expect("Failed to set config");

        jj.exec(["config", "set", "--repo", "jj-vine.gitlab.token", "token"])
            .expect("Failed to set config");

        // Set explicit MR settings (opposite of defaults)
        jj.exec([
            "config",
            "set",
            "--repo",
            "jj-vine.deleteSourceBranch",
            "false",
        ])
        .expect("Failed to set config");

        jj.exec(["config", "set", "--repo", "jj-vine.squashCommits", "true"])
            .expect("Failed to set config");

        jj.exec(["config", "set", "--repo", "jj-vine.forge", "gitlab"])
            .expect("Failed to set config");

        let config = Config::load(&repo_path).expect("Failed to load config");

        assert!(!config.delete_source_branch);
        assert!(config.squash_commits);
    }

    #[test]
    fn test_config_default_assign_to_self() {
        let (_temp, repo_path) = create_test_repo();

        let jj = Jujutsu::new(&repo_path).expect("Failed to create Jujutsu instance");
        // Set required config only
        jj.exec([
            "config",
            "set",
            "--repo",
            "jj-vine.gitlab.host",
            "https://gitlab.com",
        ])
        .expect("Failed to set config");

        jj.exec([
            "config",
            "set",
            "--repo",
            "jj-vine.gitlab.project",
            "test/proj",
        ])
        .expect("Failed to set config");

        jj.exec(["config", "set", "--repo", "jj-vine.gitlab.token", "token"])
            .expect("Failed to set config");

        jj.exec(["config", "set", "--repo", "jj-vine.forge", "gitlab"])
            .expect("Failed to set config");

        let config = Config::load(&repo_path).expect("Failed to load config");

        assert!(!config.assign_to_self);
    }

    #[test]
    fn test_config_explicit_assign_to_self() {
        let (_temp, repo_path) = create_test_repo();

        let jj = Jujutsu::new(&repo_path).expect("Failed to create Jujutsu instance");
        // Set required config
        jj.exec([
            "config",
            "set",
            "--repo",
            "jj-vine.gitlab.host",
            "https://gitlab.com",
        ])
        .expect("Failed to set config");

        jj.exec([
            "config",
            "set",
            "--repo",
            "jj-vine.gitlab.project",
            "test/proj",
        ])
        .expect("Failed to set config");

        jj.exec(["config", "set", "--repo", "jj-vine.gitlab.token", "token"])
            .expect("Failed to set config");

        // Set assign_to_self to true
        jj.exec(["config", "set", "--repo", "jj-vine.assignToSelf", "true"])
            .expect("Failed to set config");

        jj.exec(["config", "set", "--repo", "jj-vine.forge", "gitlab"])
            .expect("Failed to set config");

        let config = Config::load(&repo_path).expect("Failed to load config");

        assert!(config.assign_to_self);
    }

    #[test]
    fn test_config_default_reviewers_empty() {
        let (_temp, repo_path) = create_test_repo();

        let jj = Jujutsu::new(&repo_path).expect("Failed to create Jujutsu instance");
        // Set required config only
        jj.exec([
            "config",
            "set",
            "--repo",
            "jj-vine.gitlab.host",
            "https://gitlab.com",
        ])
        .expect("Failed to set config");

        jj.exec([
            "config",
            "set",
            "--repo",
            "jj-vine.gitlab.project",
            "test/proj",
        ])
        .expect("Failed to set config");

        jj.exec(["config", "set", "--repo", "jj-vine.gitlab.token", "token"])
            .expect("Failed to set config");

        jj.exec(["config", "set", "--repo", "jj-vine.forge", "gitlab"])
            .expect("Failed to set config");

        let config = Config::load(&repo_path).expect("Failed to load config");

        assert!(config.default_reviewers.is_empty());
    }

    #[test]
    fn test_config_default_reviewers_single() {
        let (_temp, repo_path) = create_test_repo();

        let jj = Jujutsu::new(&repo_path).expect("Failed to create Jujutsu instance");
        // Set required config
        jj.exec([
            "config",
            "set",
            "--repo",
            "jj-vine.gitlab.host",
            "https://gitlab.com",
        ])
        .expect("Failed to set config");

        jj.exec([
            "config",
            "set",
            "--repo",
            "jj-vine.gitlab.project",
            "test/proj",
        ])
        .expect("Failed to set config");

        jj.exec(["config", "set", "--repo", "jj-vine.gitlab.token", "token"])
            .expect("Failed to set config");

        // Set single reviewer as TOML array
        jj.exec([
            "config",
            "set",
            "--repo",
            "jj-vine.defaultReviewers",
            r#"["reviewer1"]"#,
        ])
        .expect("Failed to set config");

        jj.exec(["config", "set", "--repo", "jj-vine.forge", "gitlab"])
            .expect("Failed to set config");

        let config = Config::load(&repo_path).expect("Failed to load config");

        assert_eq!(config.default_reviewers, vec!["reviewer1"]);
    }

    #[test]
    fn test_config_default_reviewers_multiple() {
        let (_temp, repo_path) = create_test_repo();

        let jj = Jujutsu::new(&repo_path).expect("Failed to create Jujutsu instance");
        // Set required config
        jj.exec([
            "config",
            "set",
            "--repo",
            "jj-vine.gitlab.host",
            "https://gitlab.com",
        ])
        .expect("Failed to set config");

        jj.exec([
            "config",
            "set",
            "--repo",
            "jj-vine.gitlab.project",
            "test/proj",
        ])
        .expect("Failed to set config");

        jj.exec(["config", "set", "--repo", "jj-vine.gitlab.token", "token"])
            .expect("Failed to set config");

        // Set multiple reviewers as TOML array
        jj.exec([
            "config",
            "set",
            "--repo",
            "jj-vine.defaultReviewers",
            r#"["reviewer1", "reviewer2", "reviewer3"]"#,
        ])
        .expect("Failed to set config");

        jj.exec(["config", "set", "--repo", "jj-vine.forge", "gitlab"])
            .expect("Failed to set config");

        let config = Config::load(&repo_path).expect("Failed to load config");

        assert_eq!(
            config.default_reviewers,
            vec!["reviewer1", "reviewer2", "reviewer3"]
        );
    }

    #[test]
    fn test_gitlab_direct_mode_without_target() {
        let config = GitLabConfig {
            host: "https://gitlab.com".to_string(),
            project: "myuser/myrepo".to_string(),
            target_project: "".to_string(),
            token: "token".to_string(),
            create_merge_request_dependencies: true,
        };

        assert_eq!(config.target_project(), "myuser/myrepo");
        assert_eq!(config.source_project(), "myuser/myrepo");
        assert!(!config.is_fork_workflow());
    }

    #[test]
    fn test_gitlab_fork_mode_with_different_target() {
        let config = GitLabConfig {
            host: "https://gitlab.com".to_string(),
            project: "myuser/fork".to_string(),
            target_project: "upstream/repo".to_string(),
            token: "token".to_string(),
            create_merge_request_dependencies: true,
        };

        assert_eq!(config.target_project(), "upstream/repo");
        assert_eq!(config.source_project(), "myuser/fork");
        assert!(config.is_fork_workflow());
    }

    #[test]
    fn test_gitlab_fork_mode_with_same_target() {
        let config = GitLabConfig {
            host: "https://gitlab.com".to_string(),
            project: "myuser/repo".to_string(),
            target_project: "myuser/repo".to_string(),
            token: "token".to_string(),
            create_merge_request_dependencies: true,
        };

        assert_eq!(config.target_project(), "myuser/repo");
        assert_eq!(config.source_project(), "myuser/repo");
        assert!(!config.is_fork_workflow());
    }

    #[test]
    fn test_github_direct_mode_without_target() {
        let config = GitHubConfig {
            host: "https://api.github.com".to_string(),
            project: "myuser/myrepo".to_string(),
            target_project: "".to_string(),
            token: "token".to_string(),
        };

        assert_eq!(config.target_project(), "myuser/myrepo");
        assert_eq!(config.source_project(), "myuser/myrepo");
        assert!(!config.is_fork_workflow());
    }
}
