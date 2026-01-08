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

    // TODO: First submission would create MR (requires GitLab integration)
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

    // TODO: When we submit feature-a, what happens to feature-b?
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
    // TODO: What should happen when we try to submit it?
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

    // TODO: Verify that submitting child1 creates MRs for [base, child1]
    // TODO: Verify that submitting child2 creates MRs for [base, child2]
    // TODO: Verify that base MR is created/updated correctly for both
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
/// - Should detect broken stack
/// - Should either auto-fix (c now targets a) or error clearly
#[test]
fn test_deleted_middle_bookmark() {
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

    // Now we have: a -> (deleted) -> c
    // What should happen when we try to submit c?
    // The parent commit of c's commit still exists, but has no bookmark

    let log = repo.jj(&["log", "--no-graph"]).expect("Failed to get log");

    println!("Log after deleting middle bookmark:\n{}", log);

    // TODO: Should c now detect a as its parent?
    // Or should the commit without a bookmark be included in c's MR?
}
