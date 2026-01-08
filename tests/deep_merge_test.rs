/// Test for merge commits deeper in the history
///
/// FAILING TEST: Merge commit is not the immediate parent, but further back in history.
/// Current implementation only checks one level deep for merges.
#[path = "test_helpers.rs"]
mod test_helpers;

use jj_mrs::bookmark::BookmarkGraph;
use jj_mrs::jj::Jujutsu;
use test_helpers::TestRepo;

#[tokio::test]
async fn test_merge_commit_two_levels_deep() {
    let repo = TestRepo::new().expect("Failed to create test repo");

    // Create first branch
    repo.create_file("file1.txt", "content1")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Branch A"])
        .expect("Failed to describe");
    repo.jj(&["new"]).expect("Failed to create new change");
    repo.create_bookmark("branch-a")
        .expect("Failed to create bookmark");

    // Create second branch from root
    repo.jj(&["new", "root()"]).expect("Failed to go to root");
    repo.create_file("file2.txt", "content2")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Branch B"])
        .expect("Failed to describe");
    repo.jj(&["new"]).expect("Failed to create new change");
    repo.create_bookmark("branch-b")
        .expect("Failed to create bookmark");

    // Create a merge commit (NO BOOKMARK on the merge)
    repo.jj(&["new", "branch-a", "branch-b"])
        .expect("Failed to create merge");
    repo.create_file("merged.txt", "merged")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Merge commit"])
        .expect("Failed to describe merge");

    // Create another commit on top of the merge (still NO BOOKMARK)
    repo.jj(&["new"]).expect("Failed to create new change");
    repo.create_file("after-merge.txt", "after merge")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "After merge"])
        .expect("Failed to describe");

    // NOW create a bookmark on this commit (2 levels above the merge)
    repo.create_bookmark("feature-top")
        .expect("Failed to create bookmark");

    // Verify the structure: feature-top -> after-merge -> MERGE -> (branch-a, branch-b)
    let log = repo
        .jj(&["log", "-r", "feature-top::root()", "--no-graph"])
        .expect("Failed to get log");
    eprintln!("Log structure:\n{}", log);

    // Build the graph (should succeed - validation is separate)
    let jj = Jujutsu::new(repo.path.clone()).expect("Failed to create Jujutsu instance");
    let graph = BookmarkGraph::build(&jj, "main")
        .await
        .expect("Failed to build graph");

    // Now validate feature-top - this should detect the merge
    let result = graph.validate_bookmarks(&jj, &["feature-top".to_string()]);

    // Actually, this WILL be caught because we check immediate parent
    // feature-top -> after-merge -> MERGE (caught here!)
    match result {
        Ok(_) => {
            eprintln!("Validation passed (unexpected!): {:?}", graph.stacks);
            panic!("Should have detected merge at parent level");
        }
        Err(e) => {
            // This is correct! We DO catch merges one level deep
            eprintln!("Correctly detected merge at parent level: {}", e);
            let error_msg = format!("{}", e);
            assert!(
                error_msg.contains("merge") || error_msg.contains("multiple parents"),
                "Error should mention merge: {}",
                error_msg
            );
        }
    }
}

#[tokio::test]
async fn test_merge_commit_three_levels_deep() {
    let repo = TestRepo::new().expect("Failed to create test repo");

    // Create two branches
    repo.create_file("file1.txt", "content1")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Branch A"])
        .expect("Failed to describe");
    repo.jj(&["new"]).expect("Failed to create new change");
    repo.create_bookmark("branch-a")
        .expect("Failed to create bookmark");

    repo.jj(&["new", "root()"]).expect("Failed to go to root");
    repo.create_file("file2.txt", "content2")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Branch B"])
        .expect("Failed to describe");
    repo.jj(&["new"]).expect("Failed to create new change");
    repo.create_bookmark("branch-b")
        .expect("Failed to create bookmark");

    // Merge (no bookmark)
    repo.jj(&["new", "branch-a", "branch-b"])
        .expect("Failed to create merge");
    repo.create_file("merged.txt", "merged")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Merge"])
        .expect("Failed to describe");

    // Commit 1 after merge (no bookmark)
    repo.jj(&["new"]).expect("Failed to create new change");
    repo.create_file("after1.txt", "after1")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "After merge 1"])
        .expect("Failed to describe");

    // Commit 2 after merge (no bookmark)
    repo.jj(&["new"]).expect("Failed to create new change");
    repo.create_file("after2.txt", "after2")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "After merge 2"])
        .expect("Failed to describe");

    // Finally, bookmark (3 levels above the merge)
    repo.create_bookmark("feature-deep")
        .expect("Failed to create bookmark");

    let jj = Jujutsu::new(repo.path.clone()).expect("Failed to create Jujutsu instance");
    let graph = BookmarkGraph::build(&jj, "main")
        .await
        .expect("Failed to build graph");

    // Validate feature-deep - should detect merge 3 levels deep
    let result = graph.validate_bookmarks(&jj, &["feature-deep".to_string()]);

    match result {
        Ok(_) => {
            panic!(
                "FAILING TEST: Merge is 3 levels deep, we only check 1 level. \
                 Should have errored but validated successfully."
            );
        }
        Err(e) => {
            eprintln!("Correctly detected merge 3 levels deep: {}", e);
        }
    }
}

#[tokio::test]
async fn test_merge_between_two_bookmarks() {
    let repo = TestRepo::new().expect("Failed to create test repo");

    // Create base bookmark
    repo.create_file("base.txt", "base")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Base"])
        .expect("Failed to describe");
    repo.jj(&["new"]).expect("Failed to create new change");
    repo.create_bookmark("base")
        .expect("Failed to create bookmark");

    // Create two branches from base
    repo.create_file("branch1.txt", "branch1")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Branch 1"])
        .expect("Failed to describe");
    repo.jj(&["new"]).expect("Failed to create new change");
    repo.create_bookmark("branch1")
        .expect("Failed to create bookmark");

    repo.jj(&["new", "base"])
        .expect("Failed to go back to base");
    repo.create_file("branch2.txt", "branch2")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Branch 2"])
        .expect("Failed to describe");
    repo.jj(&["new"]).expect("Failed to create new change");
    repo.create_bookmark("branch2")
        .expect("Failed to create bookmark");

    // Merge branch1 and branch2 (no bookmark on merge)
    repo.jj(&["new", "branch1", "branch2"])
        .expect("Failed to create merge");
    repo.create_file("merged.txt", "merged")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Merge branches"])
        .expect("Failed to describe");

    // Add commit after merge, then bookmark
    repo.jj(&["new"]).expect("Failed to create new change");
    repo.create_file("top.txt", "top")
        .expect("Failed to create file");
    repo.jj(&["describe", "-m", "Top"])
        .expect("Failed to describe");
    repo.create_bookmark("top")
        .expect("Failed to create bookmark");

    // Structure:
    // base -> branch1 -\
    //                   -> MERGE -> top
    // base -> branch2 -/

    let jj = Jujutsu::new(repo.path.clone()).expect("Failed to create Jujutsu instance");
    let graph = BookmarkGraph::build(&jj, "main")
        .await
        .expect("Failed to build graph");

    // Validate top - should detect merge between bookmarks
    let result = graph.validate_bookmarks(&jj, &["top".to_string()]);

    match result {
        Ok(_) => {
            panic!("Should have detected merge between bookmarks");
        }
        Err(e) => {
            // This is correct! The merge is top's immediate parent, so we catch it
            eprintln!("Correctly detected merge between bookmarks: {}", e);
            let error_msg = format!("{}", e);
            assert!(
                error_msg.contains("merge") || error_msg.contains("multiple parents"),
                "Error should mention merge: {}",
                error_msg
            );
        }
    }
}
