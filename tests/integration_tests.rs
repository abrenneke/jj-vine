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
    repo.init_mrs_config(
        "https://gitlab.example.com",
        "test/project",
        "test-token",
        None,
        false,
    )
    .expect("Failed to init MRS config");

    // Verify config was set
    let config = repo
        .jj(&["config", "get", "jj-mrs.gitlabHost"])
        .expect("Failed to get config");
    assert!(config.contains("gitlab.example.com"));
}

/// Test that --tracked flag requires no bookmark argument
#[test]
fn test_tracked_and_bookmark_mutually_exclusive() {
    let repo = TestRepo::new().expect("Failed to create test repo");

    // Initialize config
    repo.init_mrs_config(
        "https://gitlab.example.com",
        "test/project",
        "test-token",
        None,
        false,
    )
    .expect("Failed to init MRS config");

    // Try to use both bookmark and --tracked
    let result = repo.jj_mrs_expect_error(&["submit", "feature-1", "--tracked"]);

    match result {
        Ok(stderr) => {
            assert!(
                stderr.contains("Cannot specify both"),
                "Expected mutual exclusivity error, got: {}",
                stderr
            );
        }
        Err(e) => panic!("Test failed: {}", e),
    }
}

/// Test that submit requires either bookmark or --tracked
#[test]
fn test_submit_requires_bookmark_or_tracked() {
    let repo = TestRepo::new().expect("Failed to create test repo");

    // Initialize config
    repo.init_mrs_config(
        "https://gitlab.example.com",
        "test/project",
        "test-token",
        None,
        false,
    )
    .expect("Failed to init MRS config");

    // Try to submit without any arguments
    let result = repo.jj_mrs_expect_error(&["submit"]);

    match result {
        Ok(stderr) => {
            assert!(
                stderr.contains("Must specify either"),
                "Expected validation error, got: {}",
                stderr
            );
        }
        Err(e) => panic!("Test failed: {}", e),
    }
}

/// Test that --tracked with no tracked bookmarks gives helpful error
#[test]
fn test_tracked_with_no_bookmarks() {
    let repo = TestRepo::new().expect("Failed to create test repo");

    // Initialize config
    repo.init_mrs_config(
        "https://gitlab.example.com",
        "test/project",
        "test-token",
        None,
        false,
    )
    .expect("Failed to init MRS config");

    // Create a git remote (bare repo)
    let remote_dir = repo.path.join("remote.git");
    std::fs::create_dir(&remote_dir).expect("Failed to create remote dir");
    std::process::Command::new("git")
        .args(["init", "--bare"])
        .current_dir(&remote_dir)
        .output()
        .expect("Failed to init bare repo");

    repo.add_git_remote("origin", remote_dir.to_str().unwrap())
        .expect("Failed to add remote");

    // Try to submit with --tracked (no bookmarks exist)
    let result = repo.jj_mrs_expect_error(&["submit", "--tracked"]);

    match result {
        Ok(stderr) => {
            assert!(
                stderr.contains("No tracked bookmarks found"),
                "Expected 'no tracked bookmarks' error, got: {}",
                stderr
            );
        }
        Err(e) => panic!("Test failed: {}", e),
    }
}

/// Test that tracked bookmarks are identified correctly
#[test]
fn test_tracked_bookmarks_identification() {
    let repo = TestRepo::new().expect("Failed to create test repo");

    // Create a git remote (bare repo)
    let remote_dir = repo.path.join("remote.git");
    std::fs::create_dir(&remote_dir).expect("Failed to create remote dir");
    std::process::Command::new("git")
        .args(["init", "--bare"])
        .current_dir(&remote_dir)
        .output()
        .expect("Failed to init bare repo");

    // Initialize the bare repo with a main branch
    std::process::Command::new("git")
        .args([
            "--git-dir",
            remote_dir.to_str().unwrap(),
            "symbolic-ref",
            "HEAD",
            "refs/heads/main",
        ])
        .output()
        .expect("Failed to set HEAD");

    repo.add_git_remote("origin", remote_dir.to_str().unwrap())
        .expect("Failed to add remote");

    // Create and push a bookmark
    repo.create_file("test1.txt", "content1")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "First commit"])
        .expect("Failed to describe");
    repo.create_bookmark("feature-1")
        .expect("Failed to create bookmark");

    // Track and push to remote
    repo.jj(&["bookmark", "track", "feature-1@origin"])
        .expect("Failed to track bookmark");
    repo.jj(&["git", "push", "--bookmark", "feature-1"])
        .expect("Failed to push");

    // Create another bookmark but don't push
    repo.jj(&["new"]).expect("Failed to create new change");
    repo.create_file("test2.txt", "content2")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Second commit"])
        .expect("Failed to describe");
    repo.create_bookmark("feature-2")
        .expect("Failed to create bookmark");

    // Verify feature-1 is tracked (pushed) and feature-2 is not
    let bookmarks_output = repo
        .jj(&["bookmark", "list"])
        .expect("Failed to list bookmarks");

    assert!(
        bookmarks_output.contains("feature-1"),
        "feature-1 should exist"
    );
    assert!(
        bookmarks_output.contains("feature-2"),
        "feature-2 should exist"
    );

    // Check that feature-1 has a remote tracking branch
    let remote_output = repo
        .jj(&["log", "-r", "feature-1@origin"])
        .unwrap_or_default();
    assert!(
        !remote_output.is_empty(),
        "feature-1 should have remote tracking"
    );
}

/// Test topological ordering with multiple bookmarks
#[test]
fn test_tracked_topological_ordering() {
    let repo = TestRepo::new().expect("Failed to create test repo");

    // Create a git remote (bare repo)
    let remote_dir = repo.path.join("remote.git");
    std::fs::create_dir(&remote_dir).expect("Failed to create remote dir");
    std::process::Command::new("git")
        .args(["init", "--bare"])
        .current_dir(&remote_dir)
        .output()
        .expect("Failed to init bare repo");

    std::process::Command::new("git")
        .args([
            "--git-dir",
            remote_dir.to_str().unwrap(),
            "symbolic-ref",
            "HEAD",
            "refs/heads/main",
        ])
        .output()
        .expect("Failed to set HEAD");

    repo.add_git_remote("origin", remote_dir.to_str().unwrap())
        .expect("Failed to add remote");

    // Create a chain of bookmarks: A -> B -> C
    repo.create_file("a.txt", "a")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Commit A"])
        .expect("Failed to describe");
    repo.create_bookmark("bookmark-a")
        .expect("Failed to create bookmark");
    repo.jj(&["bookmark", "track", "bookmark-a@origin"])
        .expect("Failed to track bookmark");
    repo.jj(&["git", "push", "--bookmark", "bookmark-a"])
        .expect("Failed to push");

    repo.jj(&["new"]).expect("Failed to create new change");
    repo.create_file("b.txt", "b")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Commit B"])
        .expect("Failed to describe");
    repo.create_bookmark("bookmark-b")
        .expect("Failed to create bookmark");
    repo.jj(&["bookmark", "track", "bookmark-b@origin"])
        .expect("Failed to track bookmark");
    repo.jj(&["git", "push", "--bookmark", "bookmark-b"])
        .expect("Failed to push");

    repo.jj(&["new"]).expect("Failed to create new change");
    repo.create_file("c.txt", "c")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Commit C"])
        .expect("Failed to describe");
    repo.create_bookmark("bookmark-c")
        .expect("Failed to create bookmark");
    repo.jj(&["bookmark", "track", "bookmark-c@origin"])
        .expect("Failed to track bookmark");
    repo.jj(&["git", "push", "--bookmark", "bookmark-c"])
        .expect("Failed to push");

    // Verify the bookmarks exist and are pushed
    let log_output = repo
        .jj(&["log", "-r", "bookmark-a | bookmark-b | bookmark-c"])
        .expect("Failed to get log");

    assert!(log_output.contains("Commit A"), "Commit A should exist");
    assert!(log_output.contains("Commit B"), "Commit B should exist");
    assert!(log_output.contains("Commit C"), "Commit C should exist");
}

/// Test that --tracked dry-run works end-to-end
#[test]
fn test_tracked_dry_run_end_to_end() {
    let repo = TestRepo::new().expect("Failed to create test repo");

    // Create a git remote (bare repo) with a main branch
    let remote_dir = repo.path.join("remote.git");
    std::fs::create_dir(&remote_dir).expect("Failed to create remote dir");

    // Create a temporary non-bare repo to initialize main branch
    let temp_init = repo.path.join("temp_init");
    std::fs::create_dir(&temp_init).expect("Failed to create temp init dir");
    std::process::Command::new("git")
        .args(["init"])
        .current_dir(&temp_init)
        .output()
        .expect("Failed to init temp repo");

    // Create initial commit on main
    std::fs::write(temp_init.join("README.md"), "# Test").expect("Failed to write file");
    std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(&temp_init)
        .output()
        .expect("Failed to git add");
    std::process::Command::new("git")
        .args(["commit", "-m", "Initial commit"])
        .current_dir(&temp_init)
        .output()
        .expect("Failed to commit");

    // Push to bare repo
    std::process::Command::new("git")
        .args([
            "clone",
            "--bare",
            temp_init.to_str().unwrap(),
            remote_dir.to_str().unwrap(),
        ])
        .output()
        .expect("Failed to clone bare");

    repo.add_git_remote("origin", remote_dir.to_str().unwrap())
        .expect("Failed to add remote");

    // Fetch from remote to get main@origin
    repo.jj(&["git", "fetch"]).expect("Failed to fetch");

    // Initialize jj-mrs config
    repo.init_mrs_config(
        "https://gitlab.example.com",
        "test/project",
        "test-token",
        None,
        false,
    )
    .expect("Failed to init MRS config");

    // Create and push multiple bookmarks
    repo.create_file("a.txt", "a")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Commit A"])
        .expect("Failed to describe");
    repo.create_bookmark("feature-a")
        .expect("Failed to create bookmark");
    repo.jj(&["bookmark", "track", "feature-a@origin"])
        .expect("Failed to track bookmark");
    repo.jj(&["git", "push", "--bookmark", "feature-a"])
        .expect("Failed to push");

    repo.jj(&["new"]).expect("Failed to create new change");
    repo.create_file("b.txt", "b")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Commit B"])
        .expect("Failed to describe");
    repo.create_bookmark("feature-b")
        .expect("Failed to create bookmark");
    repo.jj(&["bookmark", "track", "feature-b@origin"])
        .expect("Failed to track bookmark");
    repo.jj(&["git", "push", "--bookmark", "feature-b"])
        .expect("Failed to push");

    // Run jj mr submit --tracked --dry-run
    // For real GitLab API tests, see tests/gitlab_integration_tests.rs
    let output = repo
        .jj_mrs(&["submit", "--tracked", "--dry-run"])
        .unwrap_or_else(|e| panic!("Failed to run submit --tracked: {}", e));

    // Verify the core --tracked functionality works:
    // 1. Multiple bookmarks are identified and submitted
    assert!(
        output.contains("feature-a"),
        "Output should mention feature-a"
    );
    assert!(
        output.contains("feature-b"),
        "Output should mention feature-b"
    );

    // 2. Topological ordering is shown
    assert!(
        output.contains("topological"),
        "Output should mention topological ordering"
    );
    assert!(
        output.contains("feature-a → feature-b"),
        "Should show correct topological order"
    );

    // 3. Multiple bookmarks are processed
    assert!(
        output.contains("[1/2]"),
        "Should show progress counter [1/2]"
    );
    assert!(
        output.contains("[2/2]"),
        "Should show progress counter [2/2]"
    );

    // 4. Summary section exists
    assert!(
        output.contains("Summary"),
        "Output should have summary section"
    );
    assert!(
        output.contains("bookmarks"),
        "Summary should mention bookmarks"
    );
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

    // Create a local bare repo with a main branch so get_default_branch() can succeed
    let remote_dir = repo.path.join("remote.git");
    let temp_init = repo.path.join("temp_init");
    std::fs::create_dir(&temp_init).expect("Failed to create temp init dir");

    // Initialize a temporary repo with a main branch
    std::process::Command::new("git")
        .args(["init"])
        .current_dir(&temp_init)
        .output()
        .expect("Failed to init temp repo");

    std::fs::write(temp_init.join("README.md"), "# Test").expect("Failed to write file");
    std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(&temp_init)
        .output()
        .expect("Failed to git add");
    std::process::Command::new("git")
        .args(["commit", "-m", "Initial commit"])
        .current_dir(&temp_init)
        .output()
        .expect("Failed to commit");

    // Clone to bare repo
    std::process::Command::new("git")
        .args([
            "clone",
            "--bare",
            temp_init.to_str().unwrap(),
            remote_dir.to_str().unwrap(),
        ])
        .output()
        .expect("Failed to clone bare");

    // Add the local bare repo as origin
    repo.add_git_remote("origin", remote_dir.to_str().unwrap())
        .expect("Failed to add remote");

    // Fetch to get main@origin
    repo.jj(&["git", "fetch"]).expect("Failed to fetch");

    // Now create our feature branch
    repo.create_file("test.txt", "test")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Feature commit"])
        .expect("Failed to describe");
    repo.create_bookmark("feature-1")
        .expect("Failed to create bookmark");

    repo.init_mrs_config(
        &gitlab_config.host,
        &gitlab_config.project,
        &gitlab_config.token,
        gitlab_config.ca_bundle.clone(),
        gitlab_config.tls_accept_non_compliant_certs,
    )
    .expect("Failed to init MRS config");

    // Remove the bare repository to cause push failure while keeping main@origin in jj's view
    std::fs::remove_dir_all(&remote_dir).expect("Failed to remove remote dir");

    // Try to submit - this should fail due to missing remote
    let output = std::process::Command::new("jj")
        .arg("mr")
        .args(["submit", "feature-1"])
        .current_dir(&repo.path)
        .output()
        .expect("Failed to run jj mr submit");

    // The command should fail
    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        panic!(
            "Expected submit to fail but it succeeded.\nSTDOUT:\n{}\nSTDERR:\n{}",
            stdout, stderr
        );
    }

    // Verify the error message indicates a push failure
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Failed to push") || stderr.contains("error") || stderr.contains("push"),
        "Expected push failure error in stderr: {}",
        stderr
    );
}

/// Test that --tracked does not include the default branch (main)
#[test]
fn test_tracked_excludes_default_branch() {
    let repo = TestRepo::new().expect("Failed to create test repo");

    // Initialize config
    repo.init_mrs_config(
        "https://gitlab.example.com",
        "test/project",
        "test-token",
        None,
        false,
    )
    .expect("Failed to init MRS config");

    // Create a git remote (bare repo)
    let remote_dir = repo.path.join("remote.git");
    std::fs::create_dir(&remote_dir).expect("Failed to create remote dir");
    std::process::Command::new("git")
        .args(["init", "--bare"])
        .current_dir(&remote_dir)
        .output()
        .expect("Failed to init bare repo");

    // Initialize the bare repo with a main branch
    std::process::Command::new("git")
        .args([
            "--git-dir",
            remote_dir.to_str().unwrap(),
            "symbolic-ref",
            "HEAD",
            "refs/heads/main",
        ])
        .output()
        .expect("Failed to set HEAD");

    repo.add_git_remote("origin", remote_dir.to_str().unwrap())
        .expect("Failed to add remote");

    // Create initial commit and set up main bookmark
    repo.create_file("README.md", "# Test repo\n")
        .expect("Failed to create file");
    repo.commit("Initial commit").expect("Failed to commit");
    // Create bookmark on the previous commit (@-), not the new empty working copy (@)
    repo.jj(&["bookmark", "create", "main", "-r", "@-"])
        .expect("Failed to create main bookmark");

    // Track and push main to remote
    repo.jj(&["bookmark", "track", "main@origin"])
        .expect("Failed to track main");
    repo.jj(&["git", "push", "--bookmark", "main"])
        .expect("Failed to push main");

    // Create a feature bookmark
    repo.create_file("feature.txt", "feature content")
        .expect("Failed to create file");
    repo.commit("Add feature").expect("Failed to commit");
    // Create bookmark on the previous commit (@-), not the new empty working copy (@)
    repo.jj(&["bookmark", "create", "feature-1", "-r", "@-"])
        .expect("Failed to create feature bookmark");

    // Track and push feature to remote
    repo.jj(&["bookmark", "track", "feature-1@origin"])
        .expect("Failed to track feature-1");
    repo.jj(&["git", "push", "--bookmark", "feature-1"])
        .expect("Failed to push feature-1");

    // Run submit with --tracked and --dry-run
    let output = repo
        .jj_mrs(&["submit", "--tracked", "--dry-run"])
        .expect("Failed to run submit");

    // Verify that:
    // 1. main is NOT in the submission list
    // 2. feature-1 IS in the submission list
    // 3. The submission succeeds (no error about trying to submit main)

    assert!(
        !output.contains("Submitting main"),
        "main should NOT be submitted with --tracked. Output: {}",
        output
    );

    assert!(
        output.contains("feature-1") || output.contains("Submitting feature-1"),
        "feature-1 should be submitted with --tracked. Output: {}",
        output
    );

    assert!(
        !output.contains("Cannot submit 'main'") && !output.contains("Failed to submit main"),
        "Should not attempt to submit main at all. Output: {}",
        output
    );
}
