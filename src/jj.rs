#[cfg(test)]
use std::collections::{BTreeMap, BTreeSet};
use std::{cell::OnceCell, path::PathBuf, process::Command};

use serde::{Deserialize, Serialize};
use snafu::{OptionExt, ResultExt, whatever};
use tracing::trace;

#[cfg(test)]
use crate::bookmark::Bookmark;
use crate::{
    bookmark::JJName,
    error::{ConfigSnafu, Error, JjCommandSnafu, JsonSnafu, ParseSnafu, Result, make_whatever},
    utils::Only,
};

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

    /// The change description
    pub description: String,

    /// The IDs of the parent commits
    pub parent_commit_ids: Vec<String>,

    /// The bookmarks that are part of this change
    pub bookmarks: Vec<BookmarkInfo>,
}

impl Change {
    pub fn description_first_line(&self) -> &str {
        self.description.lines().next().unwrap_or_default().trim()
    }

    pub fn description_not_first_line(&self) -> &str {
        // Avoids allocating a String which lines().skip(1).join() would do. 🤷
        match (self.description.find('\n'), self.description.find("\r\n")) {
            (Some(n_index), Some(rn_index)) => {
                &self.description[usize::min(n_index, rn_index) + 1..]
            }
            (Some(n_index), None) => &self.description[n_index + 1..],
            (None, Some(rn_index)) => &self.description[rn_index + 2..],
            (None, None) => &self.description,
        }
        .trim()
    }
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeMap(BTreeMap<String, Change>);

#[cfg(test)]
impl ChangeMap {
    pub fn new() -> Self {
        Self(BTreeMap::new())
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
    type Target = BTreeMap<String, Change>;

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

    pub fn create_bookmark_map(&self) -> BTreeMap<String, Bookmark<'_>> {
        self.values()
            .flat_map(|change| {
                change
                    .bookmarks
                    .iter()
                    .map(|info| (info.name().to_string(), Bookmark { info, change }))
            })
            .collect()
    }

    pub fn create_adjacency_list(&self) -> BTreeMap<String, BTreeSet<String>> {
        let mut adjacency_list = BTreeMap::new();

        for (_, change) in self.iter() {
            let mut to_process: Vec<_> = change
                .parent_commit_ids
                .iter()
                .map(|id| self.get(id).unwrap())
                .collect();

            let mut parent_bookmarks = Vec::new();

            while let Some(parent) = to_process.pop() {
                match &parent.bookmarks[..] {
                    [] => {
                        to_process.extend(
                            parent
                                .parent_commit_ids
                                .iter()
                                .map(|id| self.get(id).unwrap()),
                        );
                    }
                    [info] => {
                        parent_bookmarks.push(info.full_name().to_string());
                    }
                    _ => panic!("Not supported yet"),
                }
            }

            for info in &change.bookmarks {
                adjacency_list
                    .entry(info.name().to_string())
                    .or_insert(BTreeSet::new())
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
            description: format!("description_{}", change_id),
            parent_commit_ids: vec![],
            bookmarks: vec![],
        }
    }

    /// Create a mock change from a bookmark.
    pub fn mock_from_bookmark(bookmark: &str) -> Self {
        Self {
            commit_id: format!("commit_{}", bookmark),
            change_id: format!("change_{}", bookmark),
            description: format!("description_{}", bookmark),
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
impl FromIterator<Change> for BTreeMap<String, Change> {
    fn from_iter<T: IntoIterator<Item = Change>>(iter: T) -> Self {
        iter.into_iter().map(|c| (c.commit_id.clone(), c)).collect()
    }
}

/// Jujutsu subprocess interface
pub struct Jujutsu {
    /// The directory to run all jj commands from
    cwd: PathBuf,

    /// The default branch name
    default_branch: OnceCell<Result<String, Error>>,
}

impl Jujutsu {
    /// Create a new Jujutsu instance for the given working directory
    pub fn new(cwd: impl Into<PathBuf>) -> Result<Self> {
        Self::which()?;
        Ok(Self {
            cwd: cwd.into(),
            default_branch: OnceCell::new(),
        })
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
                        description: self_commit.description,
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
    pub fn track_bookmark(&self, bookmark: impl JJName, remote: Option<&str>) -> Result<()> {
        let remote = remote.unwrap_or("origin");
        self.exec([
            "bookmark",
            "track",
            &bookmark.name_for_jj(),
            "--remote",
            remote,
        ])
        .map(|_| ())
    }

    /// Push a bookmark to a remote using jj git push. This will automatically
    /// track the bookmark on the remote if it's not already tracked
    pub fn push_bookmark(
        &self,
        bookmark: impl JJName + Copy,
        remote: Option<&str>,
    ) -> Result<bool> {
        // Try to track the bookmark first (ignore errors if already tracked)
        let _ = self.track_bookmark(bookmark, remote);

        let remote = remote.unwrap_or("origin");

        let output = self.exec([
            "git",
            "push",
            "--remote",
            remote,
            "--bookmark",
            &bookmark.name_for_jj(),
        ])?;

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

    pub fn default_branch(&self) -> Result<&str> {
        Ok(self
            .default_branch
            .get_or_init(|| {
                // First we'll try to resolve `trunk()` to a single commit.
                let output = self.log("trunk()")?.only().context(JjCommandSnafu {
                    message: "trunk() returned multiple commits!".to_string(),
                    output: None,
                })?;

                match &output.bookmarks[..] {
                    // If trunk() has no bookmarks, there's not much we can do.
                    [] => whatever!("`jj log -r 'trunk()'` returned a commit with no bookmarks!"),
                    // If trunk() has a single bookmark, that's easy
                    [bookmark] => Ok(bookmark.name().to_string()),
                    // If there are multiple bookmarks at trunk(), the next best approach is trying to parse the
                    // `revset-aliases.trunk()` jj alias as a bookmark. A user can totally override this to whatever
                    // they want, but most commonly it will be of the format `bookmark-name@remote` - and jj configures
                    // this automatically when cloning a repository, if the repository has an unusual default branch name.
                    _ => match self.exec(["config", "get", r#"revset-aliases."trunk()""#]) {
                        Ok(alias) => {
                            let bookmark: BookmarkInfo = alias.stdout.trim().parse()?;
                            Ok(bookmark.name().to_string())
                        }
                        // If we fail to parse the `revset-aliases.trunk()` alias as a bookmark, we'll fall back to a
                        // list of common branch names, in the same order that jj tries to resolve `trunk()` in the
                        // absence of a `revset-aliases.trunk()` alias.
                        Err(_) => ["main", "master", "trunk"]
                            .into_iter()
                            .find_map(|b| {
                                if output.bookmarks.iter().any(|b2| b2.name() == b) {
                                    Some(b.to_string())
                                } else {
                                    None
                                }
                            })
                            .ok_or_else(|| {
                                // At this point, we _could_ try to figure out which bookmark in the list is "the default
                                // bookmark for the default origin" per jj documentation - but the above is probably good enough for now.
                                make_whatever!("Could not identify the default branch name. Try setting the `revset-aliases.trunk()` config option or set the `jj-vine.default_base_branch` config option explicitly.")
                            }),
                    },
                }
            })
            .as_ref().map_err::<Error, _>(|e| make_whatever!("{}", e.to_string()))?
            .as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct JJCommit {
    pub commit_id: String,
    pub parents: Vec<String>,
    pub change_id: String,
    pub description: String,
    pub author: JJAuthor,
    pub committer: JJAuthor,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct JJAuthor {
    pub name: String,
    pub email: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct JJBookmark {
    pub name: String,
    pub remote: Option<String>,
    pub target: Vec<Option<String>>,
    pub tracking_target: Option<Vec<Option<String>>>,
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
        assert!(!change.description.is_empty());
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
            assert!(!change.description.is_empty());
            assert!(!change.parent_commit_ids.is_empty());
        }

        assert_eq!(changes[0].description, "Second commit\n");
        assert_eq!(changes[1].description, "First commit\n");

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
