/// Tests for merge commit handling
///
/// jj-stack explicitly rejects merge commits. We should detect them and either:
/// 1. Handle them correctly (complex)
/// 2. Reject them with a clear error message (simple)
use crate::{bookmark::BookmarkGraph, jj::Jujutsu, tests::TestRepo};

#[tokio::test]
async fn test_merge_commit_detection_in_bookmark_graph() {
    let repo = TestRepo::new();

    // Create first branch
    repo.commit_with_bookmark("file1.txt", "content1", "Branch A commit", "branch-a");

    // Go back to root and create second branch
    repo.jj(["new", "root()"]);
    repo.commit_with_bookmark("file2.txt", "content2", "Branch B commit", "branch-b");

    // Create merge commit
    repo.jj(["new", "branch-a", "branch-b"]);
    repo.create_change("merged.txt", "merged content", "Merge commit")
        .create_bookmark("merged");

    // Verify we have a merge commit
    let log_output = repo.jj([
        "log",
        "-r",
        "merged",
        "--no-graph",
        "-T",
        r#"commit_id ++ " parents: " ++ parents.len()"#,
    ]);
    assert!(
        log_output.contains("parents: 2"),
        "Should have merge commit with 2 parents"
    );

    // Build the bookmark graph (should succeed - validation is separate)
    let jj = Jujutsu::new(repo.path.clone()).expect("Failed to create Jujutsu instance");
    let bookmarks = jj.get_bookmarks().expect("Failed to get bookmarks");
    let graph = BookmarkGraph::build(&jj, "main", bookmarks)
        .await
        .expect("Failed to build graph");

    // The graph was built successfully - it only constructs the structure
    // Now validate the "merged" bookmark - this should detect the merge commit
    let result = graph.validate_bookmarks(&jj, &["merged".to_string()]);

    match result {
        Ok(_) => {
            panic!(
                "FAILING TEST: Merge commits should be rejected during validation, \
                 but validation passed."
            );
        }
        Err(e) => {
            let error_msg = format!("{}", e);
            assert!(
                error_msg.contains("merge") || error_msg.contains("multiple parents"),
                "Error should mention merge commits: {}",
                error_msg
            );
        }
    }
}

#[tokio::test]
async fn test_submit_bookmark_with_merge_in_stack() {
    let repo = TestRepo::new();

    // Create a simple linear stack
    repo.commit_with_bookmark("file1.txt", "content1", "First commit", "first");

    // Create a second branch from root
    repo.jj(["new", "root()"]);
    repo.commit_with_bookmark("file2.txt", "content2", "Second commit", "second");

    // Merge both branches
    repo.jj(["new", "first", "second"]);
    repo.commit_with_bookmark("merged.txt", "merged", "Merge both", "merged-top");

    // Build the graph (should succeed - validation is separate)
    let jj = Jujutsu::new(repo.path.clone()).expect("Failed to create Jujutsu instance");
    let bookmarks = jj.get_bookmarks().expect("Failed to get bookmarks");
    let graph = BookmarkGraph::build(&jj, "main", bookmarks)
        .await
        .expect("Failed to build graph");

    // Now validate the merged-top bookmark - should fail due to merge commit in its
    // history
    let result = graph.validate_bookmarks(&jj, &["merged-top".to_string()]);

    // The validation should fail because we detect the merge in the stack
    match result {
        Ok(_) => {
            panic!(
                "FAILING TEST: Should have errored when validating bookmark with merge commit, \
                 but validation passed."
            );
        }
        Err(e) => {
            let error_msg = format!("{}", e);
            assert!(
                error_msg.contains("merge") || error_msg.contains("multiple parents"),
                "Error should mention merge commits: {}",
                error_msg
            );
        }
    }
}
