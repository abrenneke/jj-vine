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

    // Now try to build a bookmark graph
    let jj = Jujutsu::new(repo.path.clone()).expect("Failed to create Jujutsu instance");
    let result = BookmarkGraph::build(&jj, "main").await;

    // THIS TEST WILL FAIL because we don't detect or handle merge commits
    // Expected: Either error with clear message OR handle merge correctly
    // Actual: Likely picks one parent arbitrarily, creating incorrect graph

    match result {
        Ok(graph) => {
            // The graph was built, but it's incorrect!
            // A merge commit has 2 parents, but our adjacency list only stores one
            eprintln!("Graph stacks: {:?}", graph.stacks);
            eprintln!("Adjacency list: {:?}", graph.adjacency_list);

            // BUG: This should fail because we're not handling merge commits correctly
            // The adjacency list should either:
            // 1. Contain both parents (needs data structure change), OR
            // 2. We should error when detecting a merge commit

            // For now, we'll detect the bug by checking if we only have one parent
            // when we should have two
            let parent = graph.adjacency_list.get("merged");

            // Current behavior: parent is Some("branch-b"), missing branch-a
            // Expected: Either error or represent both parents somehow
            assert!(
                parent.is_none(),
                "FAILING TEST: Merge commits should be rejected with an error, \
                 but we're currently storing only one parent: {:?}. \
                 This creates an incorrect graph structure.",
                parent
            );
        }
        Err(e) => {
            // This is actually the correct behavior!
            // We should error when encountering merge commits
            eprintln!("Correctly rejected merge commit: {}", e);
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

    // Build the graph - this should fail due to merge commit detection
    let jj = Jujutsu::new(repo.path.clone()).expect("Failed to create Jujutsu instance");
    let graph_result = BookmarkGraph::build(&jj, "main").await;

    // The graph build should fail because we detect the merge in the stack
    match graph_result {
        Ok(_) => {
            panic!(
                "FAILING TEST: Should have errored when building graph with merge commit, \
                 but graph built successfully."
            );
        }
        Err(e) => {
            // This is the correct behavior!
            eprintln!("Correctly rejected merge in stack during build: {}", e);

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
