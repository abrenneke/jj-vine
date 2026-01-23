use std::{path::PathBuf, process::Command};

use serde::{Deserialize, Serialize};
use snafu::ResultExt;
use tracing::trace;

#[cfg(test)]
use crate::bookmark::Bookmark;
use crate::error::{ConfigSnafu, Error, JjCommandSnafu, JsonSnafu, ParseSnafu, Result};

#[derive(Debug, Clone)]
pub struct CommandOutput {
    pub status: std::process::ExitStatus,
    pub stdout: String,
    pub stderr: String,
}

/// Information about a bookmark in jj. Can be local (maybe tracked) or
/// remote-only.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BookmarkInfo {
    /// A bookmark that is local to the repository, and may be also tracked
    /// against a remote repository.
    Local {
        /// The name of the bookmark, e.g. "feature/my-feature"
        name: String,

        /// Whether the bookmark is different from the local repository.
        /// In jj, an asterisk is added to the bookmark name to indicate that it
        /// is different from the local repository.
        remote_different_from_local: bool,

        /// Whether the bookmark is tracked against a remote repository.
        tracked: bool,
    },

    /// A bookmark that is only on a remote repository, and not tracked with a
    /// local bookmark.
    Remote {
        /// The name of the bookmark, e.g. "feature/my-feature"
        name: String,

        /// The remote repository name, e.g. "origin"
        remote: String,
    },
}

impl BookmarkInfo {
    /// Get the name of the bookmark. Does not include @<remote> suffix.
    pub fn name(&self) -> &str {
        match self {
            BookmarkInfo::Local { name, .. } => name,
            BookmarkInfo::Remote { name, .. } => name,
        }
    }

    /// Get the full name of the bookmark, including the @<remote> suffix if it
    /// is a remote bookmark.
    pub fn full_name(&self) -> String {
        match self {
            BookmarkInfo::Local { name, .. } => name.clone(),
            BookmarkInfo::Remote { name, remote } => format!("{}@{}", name, remote),
        }
    }

    /// Check if the bookmark is a local bookmark.
    pub fn is_local(&self) -> bool {
        matches!(self, BookmarkInfo::Local { .. })
    }

    /// Check if the bookmark is a remote bookmark.
    pub fn is_remote(&self) -> bool {
        matches!(self, BookmarkInfo::Remote { .. })
    }

    pub fn is_tracked(&self) -> bool {
        matches!(self, BookmarkInfo::Local { tracked: true, .. })
    }
}

impl std::str::FromStr for BookmarkInfo {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        if s.is_empty() {
            return Err(ParseSnafu {
                message: "Empty bookmark name".to_string(),
            }
            .build());
        }

        let remote_different_from_local = s.ends_with("*");
        let s = s.trim_end_matches("*");

        if let Some(at_pos) = s.rfind('@') {
            let name = s[..at_pos].to_string();
            let remote = s[at_pos + 1..].to_string();

            Ok(BookmarkInfo::Remote { name, remote })
        } else {
            Ok(BookmarkInfo::Local {
                name: s.to_string(),
                remote_different_from_local,
                tracked: false,
            })
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Change {
    /// Git commit ID
    pub commit_id: String,

    /// Jujutsu change ID
    pub change_id: String,

    /// The first line of the change description
    pub description_first_line: String,

    /// The IDs of the parent commits
    pub parent_commit_ids: Vec<String>,

    /// The bookmarks that are part of this change
    pub bookmarks: Vec<BookmarkInfo>,
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeMap(std::collections::BTreeMap<String, Change>);

#[cfg(test)]
impl ChangeMap {
    pub fn new() -> Self {
        Self(std::collections::BTreeMap::new())
    }

    pub fn insert(&mut self, change: Change) {
        self.0.insert(change.commit_id.clone(), change);
    }
}

#[cfg(test)]
impl Default for ChangeMap {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
impl std::ops::Deref for ChangeMap {
    type Target = std::collections::BTreeMap<String, Change>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[cfg(test)]
impl std::ops::DerefMut for ChangeMap {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[cfg(test)]
impl IntoIterator for ChangeMap {
    type Item = (String, Change);
    type IntoIter = std::collections::btree_map::IntoIter<String, Change>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

#[cfg(test)]
impl ChangeMap {
    pub fn get_bookmark(&self, bookmark: &'_ str) -> Option<Bookmark<'_>> {
        let change = self
            .values()
            .find(|c| c.bookmarks.iter().any(|b| b.name() == bookmark))?;

        Some(Bookmark {
            info: change.bookmarks.iter().find(|b| b.name() == bookmark)?,
            change,
        })
    }

    pub fn create_bookmark_map(&self) -> std::collections::BTreeMap<String, Bookmark<'_>> {
        self.iter()
            .flat_map(|(_, change)| {
                change
                    .bookmarks
                    .iter()
                    .map(|info| (info.name().to_string(), Bookmark { info, change }))
            })
            .collect()
    }

    pub fn create_adjacency_list(
        &self,
    ) -> std::collections::BTreeMap<String, std::collections::BTreeSet<String>> {
        let mut adjacency_list = std::collections::BTreeMap::new();
        for (_, change) in self.iter() {
            let parent_bookmarks = change
                .parent_commit_ids
                .iter()
                .map(|id| self.get(id).unwrap())
                .flat_map(|change| change.bookmarks.iter().map(|info| info.name().to_string()))
                .collect::<Vec<_>>();

            for info in &change.bookmarks {
                adjacency_list
                    .entry(info.name().to_string())
                    .or_insert(std::collections::BTreeSet::new())
                    .extend(parent_bookmarks.iter().cloned());
            }
        }
        adjacency_list
    }
}

#[cfg(test)]
impl Change {
    pub fn mock_stack_map(changes: impl IntoIterator<Item = Self>) -> ChangeMap {
        ChangeMap(
            Self::mock_stack(changes)
                .into_iter()
                .map(|c| (c.commit_id.clone(), c))
                .collect(),
        )
    }

    /// Create a mock stack of changes from a list of change IDs. First ID is
    /// the root.
    pub fn mock_stack(changes: impl IntoIterator<Item = Self>) -> Vec<Self> {
        let mut stack: Vec<Self> = Vec::new();
        for change in changes {
            if stack.is_empty() {
                stack.push(change.clone());
            } else {
                stack.push(change.with_mock_parent_commit_ids([
                    stack.last().as_ref().unwrap().commit_id.as_str(),
                ]));
            }
        }
        stack
    }

    /// Create a mock change from a change ID and parent change IDs.
    pub fn mock_from_change_id(change_id: &str) -> Self {
        Self {
            commit_id: format!("commit_{}", change_id),
            change_id: change_id.to_string(),
            description_first_line: format!("description_{}", change_id),
            parent_commit_ids: vec![],
            bookmarks: vec![],
        }
    }

    /// Create a mock change from a bookmark.
    pub fn mock_from_bookmark(bookmark: &str) -> Self {
        Self {
            commit_id: format!("commit_{}", bookmark),
            change_id: format!("change_{}", bookmark),
            description_first_line: format!("description_{}", bookmark),
            parent_commit_ids: vec![],
            bookmarks: vec![bookmark.parse::<BookmarkInfo>().unwrap()],
        }
    }

    pub fn with_mock_parent_commit_ids<'a>(
        mut self,
        parent_commit_ids: impl IntoIterator<Item = &'a str>,
    ) -> Self {
        self.parent_commit_ids.extend(
            parent_commit_ids
                .into_iter()
                .map(str::to_string)
                .filter(|id| !self.parent_commit_ids.contains(id))
                .collect::<Vec<_>>(),
        );
        self
    }

    pub fn with_mock_parent_bookmarks<'a>(
        mut self,
        parent_bookmarks: impl IntoIterator<Item = &'a str>,
    ) -> Self {
        let parent_bookmarks: Vec<_> = parent_bookmarks.into_iter().collect();
        self.parent_commit_ids.extend(
            parent_bookmarks
                .iter()
                .map(|id| format!("commit_{}", id))
                .filter(|id| !self.parent_commit_ids.contains(id))
                .collect::<Vec<_>>(),
        );
        self
    }

    pub fn with_mock_bookmarks<'a>(mut self, bookmarks: impl IntoIterator<Item = &'a str>) -> Self {
        self.bookmarks.extend(
            bookmarks
                .into_iter()
                .map(|b| b.parse::<BookmarkInfo>().unwrap())
                .filter(|b| !self.bookmarks.iter().any(|b2| b2.name() == b.name()))
                .collect::<Vec<_>>(),
        );
        self
    }
}

#[cfg(test)]
impl FromIterator<Change> for std::collections::BTreeMap<String, Change> {
    fn from_iter<T: IntoIterator<Item = Change>>(iter: T) -> Self {
        iter.into_iter().map(|c| (c.commit_id.clone(), c)).collect()
    }
}

/// Jujutsu subprocess interface
pub struct Jujutsu {
    /// The directory to run all jj commands from
    cwd: PathBuf,
}

impl Jujutsu {
    /// Create a new Jujutsu instance for the given working directory
    pub fn new(cwd: impl Into<PathBuf>) -> Result<Self> {
        Self::which()?;
        Ok(Self { cwd: cwd.into() })
    }

    /// Run a jj command and return the output.
    pub fn exec<'a>(&self, args: impl AsRef<[&'a str]>) -> Result<CommandOutput> {
        let args = args.as_ref();
        trace!("Running jj command: jj {}", args.join(" "));

        let jj_bin = Self::which()?;
        let output = Command::new(&jj_bin)
            .current_dir(&self.cwd)
            .args(args.as_ref())
            .output()?;

        let stderr = String::from_utf8_lossy(&output.stderr);

        if !output.status.success() {
            return Err(JjCommandSnafu {
                message: format!("jj {} failed: {}", args.as_ref().join(" "), stderr),
                output: Some(output),
            }
            .build());
        }

        // TODO interleave
        let stdout = String::from_utf8_lossy(&output.stdout);

        trace!("jj command output: {}", stdout);
        trace!("jj command stderr: {}", stderr);
        Ok(CommandOutput {
            status: output.status,
            stdout: stdout.to_string(),
            stderr: stderr.to_string(),
        })
    }

    /// Find the jj binary
    fn which() -> Result<PathBuf> {
        which::which("jj").map_err(|e| {
            ConfigSnafu {
                message: format!("jj binary not found in PATH: {}", e),
            }
            .build()
        })
    }

    /// Count the number of changes in a revset.
    pub fn count_revset(&self, revset: impl AsRef<str>) -> Result<usize> {
        Ok(self.log(revset)?.len())
    }

    /// Check if any changes exist in a revset.
    pub fn any_in_revset(&self, revset: impl AsRef<str>) -> Result<bool> {
        Ok(!self.log(revset)?.is_empty())
    }

    /// Gets information about changes in a given revset.
    pub fn log(&self, revset: impl AsRef<str>) -> Result<Vec<Change>> {
        let fields = [
            "json(self)",
            r#"json(remote_bookmarks.filter(|b| b.tracked() && b.remote() != "git"))"#,
            "json(local_bookmarks)",
        ];
        let template = fields.join(r#" ++ "\n" ++ "#) + r#"++ "\n""#;

        let output = self.exec([
            "log",
            "-r",
            revset.as_ref(),
            "--no-graph",
            "--template",
            &template,
        ])?;

        let lines: Vec<_> = output
            .stdout
            .lines()
            .filter(|line| !line.trim().is_empty())
            .collect();

        lines
            .as_slice()
            .chunks(fields.len())
            .map(|chunk| match chunk {
                [self_commit, remote_tracked_bookmarks, local_bookmarks] => {
                    let self_commit: JJCommit =
                        serde_json::from_str(self_commit).context(JsonSnafu {
                            json: self_commit.to_string(),
                        })?;

                    let remote_tracked_bookmarks: Vec<JJBookmark> =
                        serde_json::from_str(remote_tracked_bookmarks).context(JsonSnafu {
                            json: remote_tracked_bookmarks.to_string(),
                        })?;

                    let local_bookmarks: Vec<JJBookmark> = serde_json::from_str(local_bookmarks)
                        .context(JsonSnafu {
                            json: local_bookmarks.to_string(),
                        })?;

                    Ok(Change {
                        commit_id: self_commit.commit_id,
                        change_id: self_commit.change_id,
                        description_first_line: self_commit
                            .description
                            .lines()
                            .next()
                            .unwrap_or_default()
                            .to_string(),
                        parent_commit_ids: self_commit.parents,
                        bookmarks: local_bookmarks
                            .into_iter()
                            .map(|b| {
                                match remote_tracked_bookmarks.iter().find(|rt| rt.name == b.name) {
                                    Some(rt) => BookmarkInfo::Local {
                                        name: b.name,
                                        remote_different_from_local: rt.target != b.target,
                                        tracked: true,
                                    },
                                    None => BookmarkInfo::Local {
                                        name: b.name,
                                        remote_different_from_local: false,
                                        tracked: false,
                                    },
                                }
                            })
                            .collect(),
                    })
                }
                _ => Err(ParseSnafu {
                    message: format!("Failed to parse change line from jj: {:?}", chunk),
                }
                .build()),
            })
            .collect()
    }

    /// Track a bookmark on a remote.
    pub fn track_bookmark(&self, bookmark: &str, remote: Option<&str>) -> Result<()> {
        if let Some(remote) = remote {
            self.exec(["bookmark", "track", &format!("{}@{}", bookmark, remote)])?;
        } else {
            self.exec(["bookmark", "track", bookmark])?;
        }
        Ok(())
    }

    /// Push a bookmark to a remote using jj git push. This will automatically
    /// track the bookmark on the remote if it's not already tracked
    pub fn push_bookmark(&self, bookmark: &str, remote: Option<&str>) -> Result<bool> {
        // Try to track the bookmark first (ignore errors if already tracked)
        let _ = self.track_bookmark(bookmark, remote);

        let output = if let Some(remote) = remote {
            self.exec(["git", "push", "--remote", remote, "--bookmark", bookmark])?
        } else {
            self.exec(["git", "push", "--bookmark", bookmark])?
        };

        Ok(!output.stderr.contains("Nothing changed."))
    }

    /// List all remotes.
    pub fn list_remotes(&self) -> Result<Vec<String>> {
        let output = self.exec(["git", "remote", "list"])?;
        Ok(output
            .stdout
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(str::to_string)
            .collect())
    }

    /// Check if a bookmark exists on a remote.
    pub fn remote_bookmark_exists(&self, bookmark: &str, remote: Option<&str>) -> Result<bool> {
        let output = self.exec([
            "git",
            "remote",
            "list",
            "--remote",
            remote.unwrap_or("origin"),
            bookmark,
        ])?;
        Ok(!output.stdout.is_empty())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct JJCommit {
    commit_id: String,
    parents: Vec<String>,
    change_id: String,
    description: String,
    author: JJAuthor,
    committer: JJAuthor,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct JJAuthor {
    name: String,
    email: String,
    timestamp: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct JJBookmark {
    name: String,
    remote: Option<String>,
    target: Vec<Option<String>>,
    tracking_target: Option<Vec<Option<String>>>,
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::utils::Only;

    /// Create a temporary jj repository for testing
    fn create_test_repo() -> Result<(TempDir, PathBuf)> {
        let temp_dir = TempDir::new()?;
        let repo_path = temp_dir.path().to_path_buf();

        let jj = Jujutsu::new(&repo_path)?;

        jj.exec(["git", "init"])?;
        jj.exec(["config", "set", "--repo", "user.name", "Test User"])?;
        jj.exec(["config", "set", "--repo", "user.email", "test@example.com"])?;

        jj.exec(["metaedit", "--update-author"])?;

        // Create an initial commit
        std::fs::write(repo_path.join("README.md"), "# Test repo\n")?;

        let output = jj.exec(["describe", "-m", "Initial commit"])?;

        assert!(
            output.status.success(),
            "Failed to create initial commit: {}",
            output.stderr,
        );

        Ok((temp_dir, repo_path))
    }

    #[test]
    fn test_resolve_revision() -> Result<()> {
        let (_temp, repo_path) = create_test_repo()?;
        let jj = Jujutsu::new(repo_path)?;

        let change = jj.log("@")?.only().unwrap();
        assert!(!change.commit_id.is_empty());
        assert!(!change.change_id.is_empty());
        assert!(!change.description_first_line.is_empty());
        assert!(!change.parent_commit_ids.is_empty());

        Ok(())
    }

    #[test]
    fn test_bookmark_parsing() -> Result<()> {
        let bookmark = "test-feature@origin".parse::<BookmarkInfo>()?;
        match bookmark {
            BookmarkInfo::Remote { name, remote } => {
                assert_eq!(name, "test-feature");
                assert_eq!(remote, "origin");
            }
            _ => panic!("Expected remote bookmark"),
        }

        let bookmark = "test-feature".parse::<BookmarkInfo>()?;
        match bookmark {
            BookmarkInfo::Local {
                name,
                remote_different_from_local,
                tracked,
            } => {
                assert_eq!(name, "test-feature");
                assert!(!remote_different_from_local);
                assert!(!tracked);
            }
            _ => panic!("Expected local bookmark"),
        }

        let bookmark = "test-feature*".parse::<BookmarkInfo>()?;
        match bookmark {
            BookmarkInfo::Local {
                name,
                remote_different_from_local,
                tracked,
            } => {
                assert_eq!(name, "test-feature");
                assert!(remote_different_from_local);
                assert!(!tracked);
            }
            _ => panic!("Expected local bookmark"),
        };

        Ok(())
    }

    #[test]
    fn test_get_changes() -> Result<()> {
        let (_temp, repo_path) = create_test_repo()?;
        let jj = Jujutsu::new(repo_path.clone())?;

        std::fs::write(repo_path.join("test.txt"), "test content\n")?;
        jj.exec(["commit", "-m", "First commit"])?;

        std::fs::write(repo_path.join("test.txt"), "test content\n")?;
        jj.exec(["describe", "-m", "Second commit"])?;

        let changes = jj.log("root()..@")?;

        assert_eq!(changes.len(), 2);

        for change in &changes {
            assert!(!change.commit_id.is_empty());
            assert!(!change.change_id.is_empty());
            assert!(!change.description_first_line.is_empty());
            assert!(!change.parent_commit_ids.is_empty());
        }

        assert_eq!(changes[0].description_first_line, "Second commit");
        assert_eq!(changes[1].description_first_line, "First commit");

        Ok(())
    }

    #[test]
    fn test_get_tracked_bookmarks_returns_pushed() -> Result<()> {
        let (_temp, repo_path) = create_test_repo()?;
        let jj = Jujutsu::new(repo_path.clone())?;

        jj.exec(["bookmark", "create", "feature-a"])?;

        let remote_dir = _temp.path().join("remote.git");
        std::fs::create_dir(&remote_dir)?;

        let remote = Jujutsu::new(&remote_dir)?;
        remote.exec(["git", "init"])?;

        jj.exec([
            "git",
            "remote",
            "add",
            "origin",
            &remote_dir.to_string_lossy(),
        ])?;

        jj.push_bookmark("feature-a", Some("origin"))?;

        let tracked = jj.log("(mine() & tracked_remote_bookmarks()) ~ trunk()")?;

        assert_eq!(tracked.len(), 1, "Should have 1 tracked bookmark");
        assert_eq!(
            tracked[0].bookmarks[0].name(),
            "feature-a",
            "Should track feature-a"
        );

        Ok(())
    }
}
