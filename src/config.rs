use std::path::PathBuf;

use bon::Builder;
use serde::{Deserialize, de::Visitor};

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
pub enum DescriptionDiagramFormat {
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

fn default_valid_bases() -> String {
    "trunk()".to_string()
}

/// Configuration for jj-vine
#[derive(Debug, Clone, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    /// Which forge to use.
    pub forge: ForgeType,

    /// The revset to use to identify valid base bookmarks for MRs. Defaults to
    /// `trunk()`. You can change this value to widen the bookmarks that are
    /// considered valid base bookmarks. It is recommended to use the
    /// `remote_bookmarks()` function.
    ///
    /// Examples:
    /// - `remote_bookmarks("develop" | "staging" | "production")` - will
    ///   consider `develop`, `staging`, and `production` as valid base
    ///   bookmarks.
    /// - `remote_bookmarks(glob:"feature/*")` - will consider any branch name
    ///   that starts with `feature/` as valid base bookmarks.
    #[serde(default = "default_valid_bases")]
    #[builder(default = default_valid_bases())]
    pub valid_bases: String,

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

    /// Configuration for MR title generation.
    #[serde(default)]
    #[builder(default)]
    pub title: TitleConfig,

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

#[derive(Debug, Clone, Deserialize, Builder)]
#[serde(rename_all = "camelCase")]
pub struct TitleConfig {
    /// Whether to sync MR titles on every submit, or only once at MR creation.
    /// Defaults to true.
    #[serde(default = "default_true")]
    pub sync: bool,

    /// How to generate the title when an MR has only one revision.
    /// Defaults to "firstCommitFirstLine".
    #[serde(default = "default_single_revision")]
    pub single_revision: TitleFormat,

    /// How to generate the title when an MR has multiple revisions.
    #[serde(default = "default_multiple_revisions")]
    pub multiple_revisions: TitleFormat,
}

fn default_single_revision() -> TitleFormat {
    TitleFormat::FirstRevisionFirstLine
}

fn default_multiple_revisions() -> TitleFormat {
    TitleFormat::BookmarkName
}

impl Default for TitleConfig {
    fn default() -> Self {
        Self {
            sync: default_true(),
            single_revision: default_single_revision(),
            multiple_revisions: default_multiple_revisions(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum TitleFormat {
    /// Use the first revision's first line as the title.
    FirstRevisionFirstLine,

    /// Use the first revision's full message as the title.
    FirstRevisionFullMessage,

    /// Use the head revision's first line as the title.
    HeadRevisionFirstLine,

    /// Use the head revision's full message as the title.
    HeadRevisionFullMessage,

    /// Use the bookmark name as the title.
    BookmarkName,

    /// Use a custom template.
    Other(String),
}

impl<'de> Deserialize<'de> for TitleFormat {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct TitleFormatVisitor;

        impl<'de> Visitor<'de> for TitleFormatVisitor {
            type Value = TitleFormat;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a title format")
            }

            fn visit_str<E>(self, v: &str) -> std::result::Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(match v {
                    "firstRevisionFirstLine" => TitleFormat::FirstRevisionFirstLine,
                    "firstRevisionFullMessage" => TitleFormat::FirstRevisionFullMessage,
                    "headRevisionFirstLine" => TitleFormat::HeadRevisionFirstLine,
                    "headRevisionFullMessage" => TitleFormat::HeadRevisionFullMessage,
                    "bookmarkName" => TitleFormat::BookmarkName,
                    _ => TitleFormat::Other(v.to_string()),
                })
            }
        }

        deserializer.deserialize_string(TitleFormatVisitor)
    }
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

    /// If true, jj-vine will create dependencies between pull/merge requests,
    /// requiring that all parent pull/merge requests are merged before the
    /// child pull/merge request can be merged.
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

    /// Prefix for WIP pull/merge requests.
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

fn default_description_single_revision() -> DescriptionMode {
    DescriptionMode::NotFirstLine
}

fn default_description_multiple_revisions() -> DescriptionMode {
    DescriptionMode::CommitListFull
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DescriptionConfig {
    /// Whether to enable or disable description generation entirely.
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Whether to sync the description of a pull/merge request every time the
    /// bookmark is submitted. Defaults to false.
    #[serde(default)]
    pub sync: bool,

    /// How to handle the description for pull/merge
    /// requests, when there is only one revision in the pull/merge request. By
    /// default, includes the first line of the commit message.
    #[serde(default = "default_description_single_revision")]
    pub single_revision: DescriptionMode,

    /// How to handle the description for pull/merge
    /// requests, when there are multiple revisions in the pull/merge request.
    /// By default, includes the full commit messages of all revisions.
    #[serde(default = "default_description_multiple_revisions")]
    pub multiple_revisions: DescriptionMode,

    /// How to render the description for different types of pull/merge request
    /// stacks.
    #[serde(default)]
    pub diagram: DescriptionDiagramConfig,
}

impl Default for DescriptionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            sync: false,
            diagram: Default::default(),
            single_revision: DescriptionMode::NotFirstLine,
            multiple_revisions: DescriptionMode::CommitListFull,
        }
    }
}

#[derive(Debug, Clone)]
pub enum DescriptionMode {
    /// Do not render a description.
    None,

    /// Render the message of the head commit in the branch, but
    /// not the first line (because that is already used for the title).
    NotFirstLine,

    /// Render the full message of the head commit in the branch.
    FullMessage,

    /// Render a list of all commits in the branch, with their
    /// hashes and the first line of each commit message.
    CommitListFirstLine,

    /// Render a list of all commits in the branch, with their
    /// hashes and full commit messages.
    CommitListFull,

    /// Include the contents of a file at the given path as the description.
    File(String),
}

impl<'de> Deserialize<'de> for DescriptionMode {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct DescriptionModeVisitor;
        impl<'de> Visitor<'de> for DescriptionModeVisitor {
            type Value = DescriptionMode;
            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a description mode")
            }

            fn visit_str<E>(self, v: &str) -> std::result::Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                match v {
                    "none" => Ok(DescriptionMode::None),
                    "notFirstLine" => Ok(DescriptionMode::NotFirstLine),
                    "fullMessage" => Ok(DescriptionMode::FullMessage),
                    "commitListFirstLine" => Ok(DescriptionMode::CommitListFirstLine),
                    "commitListFull" => Ok(DescriptionMode::CommitListFull),
                    mode if mode.starts_with("file(") && mode.ends_with(")") => {
                        Ok(DescriptionMode::File(
                            mode.trim_start_matches("file(")
                                .trim_end_matches(")")
                                .to_string(),
                        ))
                    }
                    _ => Err(E::custom(format!("invalid description mode: {v}"))),
                }
            }
        }

        deserializer.deserialize_string(DescriptionModeVisitor)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DescriptionDiagramConfig {
    /// How to render a single pull/merge request, without any parents or
    /// children besides the trunk. Defaults to not rendering a description.
    pub single: DescriptionDiagramFormat,

    /// How to render a linear stack of MRs.
    /// Defaults to a linear numbered list.
    pub linear: DescriptionDiagramFormat,

    /// How to render a tree of MRs, where two MRs merge into a common parent.
    /// Defaults to a linear numbered list.
    pub tree: DescriptionDiagramFormat,

    /// How to render a complex graph of MRs, where two MRs merge into a common
    /// parent, or any pull/merge request has multiple parents.
    /// Defaults to a linear numbered list.
    pub complex: DescriptionDiagramFormat,
}

impl Default for DescriptionDiagramConfig {
    fn default() -> Self {
        Self {
            single: DescriptionDiagramFormat::None,
            linear: DescriptionDiagramFormat::Linear,
            tree: DescriptionDiagramFormat::Linear,
            complex: DescriptionDiagramFormat::Linear,
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
            config.description.diagram.single,
            DescriptionDiagramFormat::None
        ));
        assert!(matches!(
            config.description.diagram.linear,
            DescriptionDiagramFormat::Linear
        ));
        assert!(matches!(
            config.description.diagram.tree,
            DescriptionDiagramFormat::Linear
        ));
        assert!(matches!(
            config.description.diagram.complex,
            DescriptionDiagramFormat::Linear
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
            config.description.diagram.single,
            DescriptionDiagramFormat::None
        ));
        assert!(matches!(
            config.description.diagram.linear,
            DescriptionDiagramFormat::Linear
        ));
        assert!(matches!(
            config.description.diagram.tree,
            DescriptionDiagramFormat::Linear
        ));
        assert!(matches!(
            config.description.diagram.complex,
            DescriptionDiagramFormat::Linear
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
