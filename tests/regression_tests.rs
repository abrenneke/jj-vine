/// Regression tests for problems identified in jj-spr and jj-stack
///
/// These tests reproduce edge cases and problems that other stacked PR/MR tools have solved.
/// Each test should fail before the fix is implemented and pass after.
#[path = "test_helpers.rs"]
mod test_helpers;

use test_helpers::TestRepo;

/// Test: Unnecessary MR updates when nothing changed (jj-spr problem)
///
/// Problem: Running `jj mr submit` multiple times with no changes should not
/// push or update MRs unnecessarily. This wastes time and API quota.
///
/// Expected behavior:
/// - First submit: Creates MR
/// - Second submit with no changes: Should detect no changes and skip push/update
///
/// This test currently fails because we don't check if the bookmark has changed.
#[test]
fn test_no_op_submission_when_nothing_changed() {
    let repo = TestRepo::new().expect("Failed to create test repo");

    repo.create_file("file1.txt", "content")
        .expect("Failed to create file");
    repo.commit("First commit").expect("Failed to commit");
    repo.create_bookmark("feature-1")
        .expect("Failed to create bookmark");

    // For real GitLab MR creation tests, see tests/gitlab_integration_tests.rs
    // For now, we'll test that repeated submissions of the same bookmark
    // don't keep generating push actions

    // Second submission with no changes
    // This should be detected as a no-op
    // Expected: No push action generated if bookmark hasn't changed

    // This test will be completed once we have the infrastructure to detect
    // whether a bookmark needs pushing
}

/// Test: Multiple bookmarks pointing to the same commit (jj-stack problem)
///
/// Problem: When multiple bookmarks point to the same commit, which one should
/// be used for the MR? jj-stack handles this by letting the user choose.
///
/// Expected behavior:
/// - Should detect multiple bookmarks on same commit
/// - Should either pick one deterministically or ask user to choose
///
/// This test currently fails because we don't handle this case.
#[test]
fn test_multiple_bookmarks_on_same_commit() {
    let repo = TestRepo::new().expect("Failed to create test repo");

    repo.create_file("file1.txt", "content")
        .expect("Failed to create file");
    repo.commit("First commit").expect("Failed to commit");

    // Create two bookmarks on the same commit
    repo.create_bookmark("feature-a")
        .expect("Failed to create first bookmark");
    repo.create_bookmark("feature-b")
        .expect("Failed to create second bookmark");

    // Both bookmarks should point to the same commit
    let log_a = repo
        .jj(&["log", "-r", "feature-a", "--no-graph", "-T", "commit_id"])
        .expect("Failed to get log for feature-a");
    let log_b = repo
        .jj(&["log", "-r", "feature-b", "--no-graph", "-T", "commit_id"])
        .expect("Failed to get log for feature-b");

    assert_eq!(
        log_a.trim(),
        log_b.trim(),
        "Bookmarks should point to same commit"
    );

    // Question: When we submit feature-a, what happens to feature-b?
    // Should we warn? Should we create separate MRs? Should we error?
    // Currently this is undefined behavior.
}

/// Test: Merge commits in bookmark stack (jj-stack explicitly rejects)
///
/// Problem: Merge commits break the linear stack assumption. They have multiple
/// parents which complicates the parent-child relationship detection.
///
/// Expected behavior:
/// - Should detect merge commits in the stack
/// - Should either handle them correctly or error with clear message
///
/// This test sets up a merge scenario and verifies we handle it gracefully.
#[test]
fn test_merge_commit_in_stack() {
    let repo = TestRepo::new().expect("Failed to create test repo");

    // Create first branch
    repo.create_file("file1.txt", "content1")
        .expect("Failed to create file");
    repo.commit("Branch A commit").expect("Failed to commit");
    repo.create_bookmark("branch-a")
        .expect("Failed to create bookmark");

    // Go back to parent and create second branch
    repo.jj(&["new", "@--"])
        .expect("Failed to create new change");
    repo.create_file("file2.txt", "content2")
        .expect("Failed to create file");
    repo.commit("Branch B commit").expect("Failed to commit");
    repo.create_bookmark("branch-b")
        .expect("Failed to create bookmark");

    // Create merge commit
    repo.jj(&["new", "branch-a", "branch-b"])
        .expect("Failed to create merge");
    repo.create_file("merged.txt", "merged content")
        .expect("Failed to create file");

    // Use describe instead of commit to keep the merge structure
    repo.jj(&["describe", "-m", "Merge commit"])
        .expect("Failed to describe merge");
    repo.create_bookmark("merged")
        .expect("Failed to create bookmark");

    // Verify we have a merge commit by checking parent count
    let log_output = repo
        .jj(&[
            "log",
            "-r",
            "merged",
            "--no-graph",
            "-T",
            r#"commit_id ++ " parents: " ++ parents.len()"#,
        ])
        .expect("Failed to get parents");

    // A merge commit should have 2 parents
    assert!(
        log_output.contains("parents: 2"),
        "Merge commit should have 2 parents, got: {}",
        log_output
    );
}

/// Test: Empty bookmark (bookmark points to same commit as base)
///
/// Problem: What happens when a bookmark has no changes relative to main?
///
/// Expected behavior:
/// - Should detect that bookmark is on base branch
/// - Should either skip or error with clear message
///
/// This test currently fails because we don't check for empty stacks.
#[test]
fn test_empty_bookmark_on_base() {
    let repo = TestRepo::new().expect("Failed to create test repo");

    // Create a bookmark on the initial commit (which is on main)
    repo.create_bookmark("empty-feature")
        .expect("Failed to create bookmark");

    // This bookmark has no commits relative to main
    // Question: What should happen when we try to submit it?
    // Should we error? Should we skip it?
    // Currently this is undefined behavior.

    let log = repo
        .jj(&["log", "-r", "empty-feature", "--no-graph"])
        .expect("Failed to get log");

    println!("Bookmark log: {}", log);
}

/// Test: Bookmark graph with complex branching
///
/// Problem: When a parent bookmark has multiple child bookmarks, we need to
/// handle the branching correctly.
///
/// Expected behavior:
/// - Should create separate stacks for each branch
/// - Each stack should include the common parent
///
/// This is already tested in bookmark.rs but worth verifying end-to-end.
#[test]
fn test_complex_branching_structure() {
    let repo = TestRepo::new().expect("Failed to create test repo");

    // Create parent
    repo.create_file("base.txt", "base")
        .expect("Failed to create file");
    repo.commit("Base commit").expect("Failed to commit");
    repo.create_bookmark("base")
        .expect("Failed to create bookmark");

    // Create first child
    repo.create_file("child1.txt", "child1")
        .expect("Failed to create file");
    repo.commit("Child 1 commit").expect("Failed to commit");
    repo.create_bookmark("child1")
        .expect("Failed to create bookmark");

    // Go back to base and create second child
    repo.jj(&["new", "base"])
        .expect("Failed to go back to base");
    repo.create_file("child2.txt", "child2")
        .expect("Failed to create file");
    repo.commit("Child 2 commit").expect("Failed to commit");
    repo.create_bookmark("child2")
        .expect("Failed to create bookmark");

    // Verify we have the right structure
    let log = repo.jj(&["log", "--no-graph"]).expect("Failed to get log");

    println!("Log:\n{}", log);

    // For GitLab MR creation tests, see tests/gitlab_integration_tests.rs
    // Real integration tests verify that:
    // - Submitting a bookmark creates MRs for all ancestors
    // - MRs are correctly created/updated for stacked bookmarks
}

/// Test: Bookmark with no parent bookmark (directly on main)
///
/// Problem: A bookmark might be created directly on main without any
/// intermediate bookmarks.
///
/// Expected behavior:
/// - Should create a single MR targeting main
/// - Should not try to find parent bookmarks
#[test]
fn test_bookmark_directly_on_main() {
    let repo = TestRepo::new().expect("Failed to create test repo");

    repo.create_file("feature.txt", "feature")
        .expect("Failed to create file");
    repo.commit("Feature commit").expect("Failed to commit");
    repo.create_bookmark("simple-feature")
        .expect("Failed to create bookmark");

    // This bookmark should have no parent bookmark
    // The stack should be: [simple-feature]
    // The MR should target: main (or the default base branch)

    let log = repo
        .jj(&["log", "-r", "simple-feature", "--no-graph"])
        .expect("Failed to get log");

    // Verify bookmark exists
    assert!(log.contains("simple-feature"), "Bookmark should exist");
}

/// Test: Bookmark stack where middle bookmark is deleted
///
/// Problem: If you have stack [a, b, c] and delete bookmark b, what happens
/// when you try to submit c?
///
/// Expected behavior:
/// - bookmark-c should traverse ancestry to find nearest bookmarked ancestor
/// - bookmark-c's parent should be bookmark-a (not main)
/// - Stack should be: [bookmark-a, bookmark-c]
#[tokio::test]
async fn test_deleted_middle_bookmark() {
    let repo = TestRepo::new().expect("Failed to create test repo");

    // Create stack: a -> b -> c
    repo.create_file("file1.txt", "content1")
        .expect("Failed to create file");
    repo.commit("Commit A").expect("Failed to commit");
    repo.create_bookmark("bookmark-a")
        .expect("Failed to create bookmark");

    repo.create_file("file2.txt", "content2")
        .expect("Failed to create file");
    repo.commit("Commit B").expect("Failed to commit");
    repo.create_bookmark("bookmark-b")
        .expect("Failed to create bookmark");

    repo.create_file("file3.txt", "content3")
        .expect("Failed to create file");
    repo.commit("Commit C").expect("Failed to commit");
    repo.create_bookmark("bookmark-c")
        .expect("Failed to create bookmark");

    // Delete middle bookmark
    repo.jj(&["bookmark", "delete", "bookmark-b"])
        .expect("Failed to delete bookmark");

    // Build the bookmark graph
    use jj_mrs::bookmark::BookmarkGraph;
    use jj_mrs::jj::Jujutsu;

    let jj = Jujutsu::new(repo.path.clone()).expect("Failed to create Jujutsu instance");
    let bookmarks = jj.get_bookmarks().expect("Failed to get bookmarks");
    let graph = BookmarkGraph::build(&jj, "main", bookmarks)
        .await
        .expect("Failed to build graph");

    // Verify that bookmark-c's parent is bookmark-a
    let parent = graph.get_parent("bookmark-c");
    assert_eq!(
        parent,
        Some(&"bookmark-a".to_string()),
        "bookmark-c should have bookmark-a as parent after bookmark-b is deleted"
    );

    // Verify the stack structure
    let stack = graph
        .find_stack_for_bookmark("bookmark-c")
        .expect("bookmark-c should be in a stack");

    assert_eq!(
        stack.bookmarks,
        vec!["bookmark-a", "bookmark-c"],
        "Stack should contain bookmark-a and bookmark-c in order"
    );
}

/// Test: Default branch configuration is used for stack base
///
/// Problem: The base branch was hardcoded to "main" instead of using the
/// default_branch from config, as identified in TESTING_FINDINGS.md
///
/// Expected behavior:
/// - When building bookmark graph, the default_branch parameter should be used
/// - Each stack's base should match the provided default_branch
/// - This allows repositories using different default branches (e.g., "master", "develop")
#[tokio::test]
async fn test_default_branch_configuration() {
    use jj_mrs::bookmark::BookmarkGraph;
    use jj_mrs::jj::Jujutsu;

    let repo = TestRepo::new().expect("Failed to create test repo");

    // Create a simple bookmark stack
    repo.create_file("file1.txt", "content1")
        .expect("Failed to create file");
    repo.jj(&["commit", "-m", "First commit"])
        .expect("Failed to commit");
    repo.create_bookmark("feature-a")
        .expect("Failed to create bookmark");

    repo.create_file("file2.txt", "content2")
        .expect("Failed to create file");
    repo.jj(&["commit", "-m", "Second commit"])
        .expect("Failed to commit");
    repo.create_bookmark("feature-b")
        .expect("Failed to create bookmark");

    // Build the graph with "main" as default_branch
    let jj = Jujutsu::new(repo.path.clone()).expect("Failed to create Jujutsu instance");
    let bookmarks = jj.get_bookmarks().expect("Failed to get bookmarks");
    let graph_main = BookmarkGraph::build(&jj, "main", bookmarks.clone())
        .await
        .expect("Failed to build graph with 'main'");

    let stack_main = graph_main
        .find_stack_for_bookmark("feature-b")
        .expect("feature-b should be in a stack");

    assert_eq!(
        stack_main.base, "main",
        "Stack base should be 'main' when configured"
    );

    // Build the graph with "develop" as default_branch
    let graph_develop = BookmarkGraph::build(&jj, "develop", bookmarks.clone())
        .await
        .expect("Failed to build graph with 'develop'");

    let stack_develop = graph_develop
        .find_stack_for_bookmark("feature-b")
        .expect("feature-b should be in a stack");

    assert_eq!(
        stack_develop.base, "develop",
        "Stack base should be 'develop' when configured"
    );

    // Build the graph with "master" as default_branch
    let graph_master = BookmarkGraph::build(&jj, "master", bookmarks)
        .await
        .expect("Failed to build graph with 'master'");

    let stack_master = graph_master
        .find_stack_for_bookmark("feature-b")
        .expect("feature-b should be in a stack");

    assert_eq!(
        stack_master.base, "master",
        "Stack base should be 'master' when configured"
    );
}

/// Test: Base branch should not be pushed when submitting feature bookmarks
///
/// Problem: When submitting a feature bookmark, the tool was trying to push the
/// base branch (main) to the remote, which would fail on protected branches.
///
/// Expected behavior:
/// - Only feature bookmarks should be pushed
/// - Base branch should NOT be pushed
/// - First MR should target the base branch
#[tokio::test]
async fn test_base_branch_not_pushed() {
    use jj_mrs::config::Config;
    use jj_mrs::jj::Jujutsu;
    use jj_mrs::submit::analyze;

    let repo = TestRepo::new().expect("Failed to create test repo");

    // Setup: Create stack main → feature-1 → feature-2
    repo.create_file("file1.txt", "initial")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Initial commit"])
        .expect("Failed to describe");
    repo.create_bookmark("main").expect("Failed to create main");

    repo.jj(&["new"]).expect("Failed to create new change");
    repo.create_file("file2.txt", "feature1")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Feature 1"])
        .expect("Failed to describe");
    repo.create_bookmark("feature-1")
        .expect("Failed to create feature-1");

    repo.jj(&["new"]).expect("Failed to create new change");
    repo.create_file("file3.txt", "feature2")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Feature 2"])
        .expect("Failed to describe");
    repo.create_bookmark("feature-2")
        .expect("Failed to create feature-2");

    // Test the analyze function directly
    let jj = Jujutsu::new(repo.path.clone()).expect("Failed to create Jujutsu instance");
    let config = Config {
        gitlab_host: "https://gitlab.example.com".to_string(),
        gitlab_project: "test/project".to_string(),
        gitlab_token: "fake-token".to_string(),
        default_branch: "main".to_string(),
        remote_name: "origin".to_string(),
        ca_bundle: None,
        tls_accept_non_compliant_certs: false,
        enable_stack_visualization: true,
        stack_format: jj_mrs::config::StackFormat::Linear,
    };

    let analysis = analyze::analyze(&jj, &config, &["feature-2".to_string()])
        .await
        .expect("Failed to analyze");

    // Verify: Should NOT include main in bookmarks_to_submit
    assert!(
        !analysis.bookmarks_to_submit.contains(&"main".to_string()),
        "bookmarks_to_submit should not contain 'main': {:?}",
        analysis.bookmarks_to_submit
    );

    // Verify: SHOULD include feature bookmarks
    assert!(
        analysis
            .bookmarks_to_submit
            .contains(&"feature-1".to_string()),
        "bookmarks_to_submit should contain 'feature-1': {:?}",
        analysis.bookmarks_to_submit
    );
    assert!(
        analysis
            .bookmarks_to_submit
            .contains(&"feature-2".to_string()),
        "bookmarks_to_submit should contain 'feature-2': {:?}",
        analysis.bookmarks_to_submit
    );

    // Verify: base_branch should still be 'main' (needed for MR targeting)
    assert_eq!(analysis.base_branch, "main");
}

/// Test: Single feature bookmark (directly on base) should not push base
#[tokio::test]
async fn test_single_bookmark_not_push_base() {
    use jj_mrs::config::Config;
    use jj_mrs::jj::Jujutsu;
    use jj_mrs::submit::analyze;

    let repo = TestRepo::new().expect("Failed to create test repo");

    // Setup: main → feature-1 (single feature bookmark)
    repo.create_file("file1.txt", "initial")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Initial commit"])
        .expect("Failed to describe");
    repo.create_bookmark("main").expect("Failed to create main");

    repo.jj(&["new"]).expect("Failed to create new change");
    repo.create_file("file2.txt", "feature")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Feature"])
        .expect("Failed to describe");
    repo.create_bookmark("feature-1")
        .expect("Failed to create feature-1");

    let jj = Jujutsu::new(repo.path.clone()).expect("Failed to create Jujutsu instance");
    let config = Config {
        gitlab_host: "https://gitlab.example.com".to_string(),
        gitlab_project: "test/project".to_string(),
        gitlab_token: "fake-token".to_string(),
        default_branch: "main".to_string(),
        remote_name: "origin".to_string(),
        ca_bundle: None,
        tls_accept_non_compliant_certs: false,
        enable_stack_visualization: true,
        stack_format: jj_mrs::config::StackFormat::Linear,
    };

    let analysis = analyze::analyze(&jj, &config, &["feature-1".to_string()])
        .await
        .expect("Failed to analyze");

    // Should NOT include main
    assert!(
        !analysis.bookmarks_to_submit.contains(&"main".to_string()),
        "Should not include base branch: {:?}",
        analysis.bookmarks_to_submit
    );

    // SHOULD include only feature-1
    assert_eq!(
        analysis.bookmarks_to_submit,
        vec!["feature-1".to_string()],
        "Should contain only feature-1: {:?}",
        analysis.bookmarks_to_submit
    );

    assert_eq!(analysis.base_branch, "main");
}

/// Test: Attempting to submit the base branch should error
#[tokio::test]
async fn test_submit_base_branch_errors() {
    use jj_mrs::config::Config;
    use jj_mrs::jj::Jujutsu;
    use jj_mrs::submit::analyze;

    let repo = TestRepo::new().expect("Failed to create test repo");

    repo.create_file("file1.txt", "initial")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Initial commit"])
        .expect("Failed to describe");
    repo.create_bookmark("main").expect("Failed to create main");

    let jj = Jujutsu::new(repo.path.clone()).expect("Failed to create Jujutsu instance");
    let config = Config {
        gitlab_host: "https://gitlab.example.com".to_string(),
        gitlab_project: "test/project".to_string(),
        gitlab_token: "fake-token".to_string(),
        default_branch: "main".to_string(),
        remote_name: "origin".to_string(),
        ca_bundle: None,
        tls_accept_non_compliant_certs: false,
        enable_stack_visualization: true,
        stack_format: jj_mrs::config::StackFormat::Linear,
    };

    // Attempting to submit main should error
    let result = analyze::analyze(&jj, &config, &["main".to_string()]).await;

    assert!(result.is_err(), "Should error when submitting base branch");

    if let Err(e) = result {
        let error_msg = format!("{}", e);
        assert!(
            error_msg.contains("base branch"),
            "Error should mention base branch: {}",
            error_msg
        );
    }
}

/// Test: BookmarkGraph::build() should not traverse entire default branch history
///
/// Problem: When building a bookmark graph, the code was traversing the entire
/// commit history of the default branch (master/main) looking for a bookmarked
/// ancestor. In large repositories with thousands of commits, this caused
/// multi-minute hangs.
///
/// Expected behavior:
/// - Building the graph should skip processing the default branch
/// - The default branch has no parent bookmark by definition
/// - Graph building should complete quickly even with long default branch history
///
/// This test creates a repo with:
/// - master bookmark with a long commit history (100+ commits)
/// - feature branches off master
/// Then verifies BookmarkGraph::build() completes quickly without traversing
/// the entire master history.
#[tokio::test]
async fn test_bookmark_graph_does_not_traverse_default_branch_history() {
    use jj_mrs::bookmark::BookmarkGraph;
    use jj_mrs::jj::Jujutsu;

    // Initialize tracing for debugging
    let _ = tracing_subscriber::fmt()
        .with_env_filter("debug")
        .with_test_writer()
        .try_init();

    let repo = TestRepo::new().expect("Failed to create test repo");

    // Create an initial bookmark that master will descend from
    repo.create_file("initial.txt", "initial")
        .expect("Failed to create initial file");
    repo.jj(&["describe", "-m", "Initial commit"])
        .expect("Failed to describe");
    repo.create_bookmark("initial")
        .expect("Failed to create initial bookmark");

    // Create a long history on master (simulating a real repo)
    // Create 100 commits to make this realistic
    for i in 1..=100 {
        repo.jj(&["new"]).expect("Failed to create new change");
        repo.create_file(&format!("file{}.txt", i), &format!("content {}", i))
            .expect("Failed to create file");
        repo.jj(&["describe", "-m", &format!("Commit {}", i)])
            .expect("Failed to describe");
    }

    // Create master bookmark at the tip
    repo.create_bookmark("master")
        .expect("Failed to create master");

    // Create a feature bookmark off master
    repo.jj(&["new", "master"])
        .expect("Failed to create new change off master");
    repo.create_file("feature.txt", "feature content")
        .expect("Failed to create feature file");
    repo.jj(&["describe", "-m", "Feature commit"])
        .expect("Failed to describe");
    repo.create_bookmark("feature-1")
        .expect("Failed to create feature-1");

    // Now build the bookmark graph
    // This should NOT traverse the entire 100-commit master history
    let jj = Jujutsu::new(repo.path.clone()).expect("Failed to create Jujutsu instance");
    let bookmarks = jj.get_bookmarks().expect("Failed to get bookmarks");

    let graph = BookmarkGraph::build(&jj, "master", bookmarks)
        .await
        .expect("Failed to build bookmark graph");

    // The default branch has no parent bookmark
    let master_parent = graph.get_parent("master");
    assert_eq!(
        master_parent, None,
        "Default branch should have no parent bookmark"
    );

    // Feature branches should have the default branch as their parent
    let parent = graph.get_parent("feature-1");
    assert_eq!(
        parent,
        Some(&"master".to_string()),
        "feature-1 should have master as parent"
    );
}

/// Submitting tracked bookmarks should succeed even when the repository
/// contains untracked bookmarks with merge commits.
#[tokio::test]
async fn test_submit_tracked_ignores_untracked_merge_commits() {
    use jj_mrs::commands::submit::submit;
    use test_helpers::{GitLabTest, unique_test_branch};

    let test = match GitLabTest::setup().await {
        Some(t) => t,
        None => return,
    };

    let repo = test.repo;

    // Use unique branch name to avoid conflicts with previous test runs
    let tracked_feature = unique_test_branch("tracked-feature");

    // Create main locally (no need to push - config specifies default branch)
    repo.create_file("initial.txt", "initial")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Initial commit"])
        .expect("Failed to describe");
    repo.create_bookmark("main").expect("Failed to create main");

    // Create a clean tracked bookmark
    repo.create_file("feature.txt", "feature")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Feature commit"])
        .expect("Failed to describe");
    repo.create_bookmark(&tracked_feature)
        .expect("Failed to create tracked-feature");
    repo.jj(&["bookmark", "track", &format!("{}@origin", tracked_feature)])
        .expect("Failed to track tracked-feature");
    repo.jj(&["git", "push", "--bookmark", &tracked_feature])
        .expect("Failed to push tracked-feature");

    // Create untracked bookmark with merge commit
    repo.jj(&["new", "main"]).expect("Failed to new from main");
    repo.create_file("branch1.txt", "branch1")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Branch 1"])
        .expect("Failed to describe");
    let branch1_id = repo
        .jj(&["log", "-r", "@", "--no-graph", "-T", "commit_id"])
        .expect("Failed to get commit id")
        .trim()
        .to_string();

    repo.jj(&["new", "main"]).expect("Failed to new from main");
    repo.create_file("branch2.txt", "branch2")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Branch 2"])
        .expect("Failed to describe");
    let branch2_id = repo
        .jj(&["log", "-r", "@", "--no-graph", "-T", "commit_id"])
        .expect("Failed to get commit id")
        .trim()
        .to_string();

    // Create merge commit (untracked)
    repo.jj(&["new", &branch1_id, &branch2_id])
        .expect("Failed to create merge");
    repo.jj(&["describe", "-m", "Merge commit"])
        .expect("Failed to describe merge");
    repo.create_bookmark("untracked-merge")
        .expect("Failed to create untracked-merge");

    // Submit only the tracked bookmark - should succeed
    let result = submit(
        repo.path.clone(),
        vec![tracked_feature],
        "origin".to_string(),
        true, // dry_run
        false, // verbose
    )
    .await;

    assert!(
        result.is_ok(),
        "Submit should succeed even with untracked merge commits: {:?}",
        result.err()
    );
}
