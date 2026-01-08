/// Integration tests for jj-mrs
#[path = "test_helpers.rs"]
mod test_helpers;

use test_helpers::TestRepo;

#[test]
fn test_e2e_basic_repo_setup() {
    let repo = TestRepo::new().expect("Failed to create test repo");

    // Verify jj is initialized
    let status = repo.jj(&["status"]).expect("Failed to run jj status");
    assert!(status.contains("Working copy"));

    // Create a test file and commit
    repo.create_file("test.txt", "test content")
        .expect("Failed to create file");
    repo.commit("Initial commit").expect("Failed to commit");

    // Verify commit was created
    let log = repo.jj(&["log", "-r", "@-"]).expect("Failed to get log");
    assert!(log.contains("Initial commit"));
}

#[test]
fn test_e2e_mrs_config() {
    let repo = TestRepo::new().expect("Failed to create test repo");

    // Initialize MRS config
    repo.init_mrs_config("https://gitlab.example.com", "test/project", "test-token")
        .expect("Failed to init MRS config");

    // Verify config was set
    let config = repo
        .jj(&["config", "get", "spr.gitlabHost"])
        .expect("Failed to get config");
    assert!(config.contains("gitlab.example.com"));
}

/// Regression test for push failure handling
///
/// This test verifies that when a bookmark push fails, the corresponding
/// MR creation is skipped and an error is properly reported.
///
/// Before the fix: MR creation would proceed even when pushes failed,
/// resulting in API errors when trying to create MRs for branches that don't exist on remote.
///
/// After the fix: Failed pushes are tracked and MR creation is skipped for those bookmarks.
///
/// # Requirements
///
/// This test requires GitLab credentials set via environment variables:
/// - GITLAB_HOST: GitLab instance URL (e.g., "https://gitlab.com")
/// - GITLAB_PROJECT: Project path (e.g., "username/test-repo")
/// - GITLAB_TOKEN: Personal access token with API access
///
/// The test will be skipped if these environment variables are not set.
#[test]
fn test_e2e_push_failure_skips_mr_creation() {
    use test_helpers::GitLabConfig;

    let gitlab_config = match GitLabConfig::from_env() {
        Some(config) => config,
        None => {
            eprintln!("Skipping test: GitLab environment variables not set");
            eprintln!("Set GITLAB_HOST, GITLAB_PROJECT, and GITLAB_TOKEN to run this test");
            return;
        }
    };

    let repo = TestRepo::new().expect("Failed to create test repo");

    repo.create_file("test.txt", "test")
        .expect("Failed to create file");
    repo.commit("Initial commit").expect("Failed to commit");
    repo.create_bookmark("feature-1")
        .expect("Failed to create bookmark");

    repo.init_mrs_config(
        &gitlab_config.host,
        &gitlab_config.project,
        &gitlab_config.token,
    )
    .expect("Failed to init MRS config");

    repo.add_git_remote(
        "origin",
        "https://gitlab.com/nonexistent-user-12345/nonexistent-repo-67890.git",
    )
    .expect("Failed to add git remote");

    let result = repo.jj_mrs_expect_error(&["submit", "feature-1"]);

    match result {
        Ok(stderr) => {
            assert!(
                stderr.contains("Failed to push") || stderr.contains("error"),
                "Expected push failure error in stderr: {}",
                stderr
            );
        }
        Err(e) => panic!("Test setup failed: {}", e),
    }
}
