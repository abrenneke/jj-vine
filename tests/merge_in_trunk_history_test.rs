/// Test for handling merge commits that exist in trunk history
///
/// This tests the scenario where a repository has old merge commits in its
/// trunk/main/master branch history. When creating a linear bookmark on top
/// of this trunk, the validation should NOT fail because the merge commits
/// are not part of the stack being submitted - they're already in the base.
///
/// This addresses the issue where validation was checking ALL ancestors
/// including trunk history, rather than just the NEW commits.
#[path = "test_helpers.rs"]
mod test_helpers;

use jj_mrs::bookmark::BookmarkGraph;
use jj_mrs::jj::Jujutsu;
use test_helpers::TestRepo;

#[tokio::test]
async fn test_linear_bookmark_on_trunk_with_merge_history() {
    let repo = TestRepo::new().expect("Failed to create test repo");

    // Create a local bare git repo as a remote so trunk() works
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
            "refs/heads/trunk",
        ])
        .output()
        .expect("Failed to set HEAD");

    repo.add_git_remote("origin", remote_dir.to_str().unwrap())
        .expect("Failed to add remote");

    // Create a trunk branch with an old merge commit in its history
    // This simulates a real repository where master/main has merge commits

    // Create first branch for the merge (no bookmark, just a commit)
    repo.create_file("file1.txt", "content1")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Branch A commit"])
        .expect("Failed to describe");
    let branch_a_id = repo
        .jj(&["log", "-r", "@", "--no-graph", "-T", "commit_id"])
        .expect("Failed to get commit id")
        .trim()
        .to_string();

    // Go back to root and create second branch (no bookmark, just a commit)
    repo.jj(&["new", "root()"]).expect("Failed to go to root");
    repo.create_file("file2.txt", "content2")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Branch B commit"])
        .expect("Failed to describe");
    let branch_b_id = repo
        .jj(&["log", "-r", "@", "--no-graph", "-T", "commit_id"])
        .expect("Failed to get commit id")
        .trim()
        .to_string();

    // Create merge commit that will be in trunk history
    // Don't create bookmarks for the merge parents - they're just history
    repo.jj(&["new", &branch_a_id, &branch_b_id])
        .expect("Failed to create merge");
    repo.create_file("merged.txt", "merged content")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Merge commit in trunk"])
        .expect("Failed to describe merge");

    // Add more commits on top to simulate a real trunk
    repo.jj(&["new"]).expect("Failed to create new change");
    repo.create_file("post-merge1.txt", "content")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Post-merge commit 1"])
        .expect("Failed to describe");
    repo.jj(&["new"]).expect("Failed to create new change");
    repo.create_file("post-merge2.txt", "content")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Post-merge commit 2"])
        .expect("Failed to describe");

    // This becomes our "trunk" - it has a merge in its history
    // The merge itself doesn't have a bookmark, it's just in the history
    repo.create_bookmark("trunk")
        .expect("Failed to create trunk bookmark");

    // Push trunk to remote so trunk() resolves correctly
    repo.jj(&["bookmark", "track", "trunk@origin"])
        .expect("Failed to track trunk");
    repo.jj(&["git", "push", "--bookmark", "trunk"])
        .expect("Failed to push trunk");
    repo.jj(&["git", "fetch"]).expect("Failed to fetch");

    // Verify we have a merge commit in trunk's history
    let log_output = repo
        .jj(&[
            "log",
            "-r",
            "::trunk & merges()",
            "--no-graph",
            "-T",
            "description",
        ])
        .expect("Failed to get merges");
    assert!(
        log_output.contains("Merge commit in trunk"),
        "Trunk should have merge commit in its history"
    );

    // Now create a linear bookmark on top of trunk
    // This should be valid even though trunk has merges in its history
    repo.jj(&["new", "trunk"])
        .expect("Failed to create new change");
    repo.create_file("feature.txt", "feature content")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Feature commit"])
        .expect("Failed to describe");
    repo.jj(&["new"]).expect("Failed to create new change");
    repo.create_bookmark("my-feature")
        .expect("Failed to create bookmark");

    // Verify the new bookmark is linear (no merges in ::my-feature ~ ::trunk)
    let log_output = repo
        .jj(&[
            "log",
            "-r",
            "::my-feature ~ ::trunk & merges()",
            "--no-graph",
        ])
        .expect("Failed to check for merges");
    assert!(
        log_output.trim().is_empty() || !log_output.contains("Merge"),
        "The new commits should not contain any merge commits"
    );

    // Build the bookmark graph
    // Important: we only include my-feature in the graph, not trunk
    // This simulates the real scenario where trunk (master) is not in mine() & bookmarks()
    let jj = Jujutsu::new(repo.path.clone()).expect("Failed to create Jujutsu instance");
    let my_feature_bookmark = jj
        .get_bookmarks_with_revset("my-feature & bookmarks()")
        .expect("Failed to get my-feature bookmark");
    assert_eq!(
        my_feature_bookmark.len(),
        1,
        "Should have exactly one bookmark"
    );

    let graph = BookmarkGraph::build(&jj, "trunk", my_feature_bookmark)
        .await
        .expect("Failed to build graph");

    // Validate the my-feature bookmark
    // This should PASS because the merge is in trunk, not in the new commits
    let result = graph.validate_bookmarks(&jj, &["my-feature".to_string()]);

    match result {
        Ok(_) => {
            // This is the CORRECT behavior!
            // The bookmark is linear even though trunk has merges
            eprintln!("✓ Correctly validated linear bookmark on trunk with merge history");
        }
        Err(e) => {
            // This is the BUG we're testing for!
            // The validation should NOT fail just because trunk has merges
            panic!(
                "BUG: Validation failed for linear bookmark on trunk with merge history.\n\
                 The merge commit is in trunk's history, not in the new commits.\n\
                 Error: {}",
                e
            );
        }
    }
}
