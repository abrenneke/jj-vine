/// Tests for merge commit handling
///
/// jj-stack explicitly rejects merge commits. We should detect them and either:
/// 1. Handle them correctly (complex)
/// 2. Reject them with a clear error message (simple)
///
/// This test will FAIL until we implement merge commit detection.
#[path = "test_helpers.rs"]
mod test_helpers;

use jj_mrs::bookmark::BookmarkGraph;
use jj_mrs::jj::Jujutsu;
use test_helpers::TestRepo;

#[tokio::test]
async fn test_merge_commit_detection_in_bookmark_graph() {
    let repo = TestRepo::new().expect("Failed to create test repo");

    // Create first branch
    repo.create_file("file1.txt", "content1")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Branch A commit"])
        .expect("Failed to describe");
    repo.jj(&["new"]).expect("Failed to create new change");
    repo.create_bookmark("branch-a")
        .expect("Failed to create bookmark");

    // Go back to root and create second branch
    repo.jj(&["new", "root()"]).expect("Failed to go to root");
    repo.create_file("file2.txt", "content2")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Branch B commit"])
        .expect("Failed to describe");
    repo.jj(&["new"]).expect("Failed to create new change");
    repo.create_bookmark("branch-b")
        .expect("Failed to create bookmark");

    // Create merge commit
    repo.jj(&["new", "branch-a", "branch-b"])
        .expect("Failed to create merge");
    repo.create_file("merged.txt", "merged content")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Merge commit"])
        .expect("Failed to describe merge");
    repo.create_bookmark("merged")
        .expect("Failed to create bookmark");

    // Verify we have a merge commit
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
            // The graph was built, but validation should have caught the merge!
            eprintln!("Graph stacks: {:?}", graph.stacks);
            eprintln!("Adjacency list: {:?}", graph.adjacency_list);

            panic!(
                "FAILING TEST: Merge commits should be rejected during validation, \
                 but validation passed."
            );
        }
        Err(e) => {
            // This is the correct behavior!
            // We should error when validating merge commits
            eprintln!("Correctly rejected merge commit during validation: {}", e);
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
    let repo = TestRepo::new().expect("Failed to create test repo");

    // Create a simple linear stack
    repo.create_file("file1.txt", "content1")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "First commit"])
        .expect("Failed to describe");
    repo.jj(&["new"]).expect("Failed to create new change");
    repo.create_bookmark("first")
        .expect("Failed to create bookmark");

    // Create a second branch from root
    repo.jj(&["new", "root()"]).expect("Failed to go to root");
    repo.create_file("file2.txt", "content2")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Second commit"])
        .expect("Failed to describe");
    repo.jj(&["new"]).expect("Failed to create new change");
    repo.create_bookmark("second")
        .expect("Failed to create bookmark");

    // Merge both branches
    repo.jj(&["new", "first", "second"])
        .expect("Failed to create merge");
    repo.create_file("merged.txt", "merged")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Merge both"])
        .expect("Failed to describe");
    repo.jj(&["new"]).expect("Failed to create new change");
    repo.create_bookmark("merged-top")
        .expect("Failed to create bookmark");

    // Build the graph (should succeed - validation is separate)
    let jj = Jujutsu::new(repo.path.clone()).expect("Failed to create Jujutsu instance");
    let bookmarks = jj.get_bookmarks().expect("Failed to get bookmarks");
    let graph = BookmarkGraph::build(&jj, "main", bookmarks)
        .await
        .expect("Failed to build graph");

    // Now validate the merged-top bookmark - should fail due to merge commit in its history
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
            // This is the correct behavior!
            eprintln!("Correctly rejected merge in stack during validation: {}", e);

            // Verify error message is clear
            let error_msg = format!("{}", e);
            assert!(
                error_msg.contains("merge") || error_msg.contains("multiple parents"),
                "Error should mention merge commits: {}",
                error_msg
            );
        }
    }
}
