use crate::{commands::submit::SubmitCommandConfig, tests::{TestRepo, unique_branch}};

/// Test that --tracked identifies only pushed bookmarks
#[tokio::test]
async fn test_tracked_only_includes_pushed_bookmarks() {
    let repo = TestRepo::with_gitlab_remote();

    // Create and push bookmark A
    let branch_a = unique_branch("tracked-a");
    repo.jj(["new", "main"]);
    repo.create_change("a.txt", "a", "Commit A")
        .create_and_push_bookmark(&branch_a);

    // Create bookmark B but don't push it
    let branch_b = unique_branch("tracked-b");
    repo.jj(["new"]);
    repo.create_change("b.txt", "b", "Commit B")
        .create_bookmark(&branch_b);

    // Submit with --tracked --dry-run
    let output = repo
        .submit(SubmitCommandConfig {
            tracked: true,
            dry_run: true,
            ..Default::default()
        })
        .await;

    // branch_a should be in output (it's pushed)
    assert!(
        output.contains(&branch_a),
        "Pushed bookmark {} should be in --tracked output:\n{}",
        branch_a,
        output
    );

    // branch_b should NOT be in output (it's not pushed)
    assert!(
        !output.contains(&branch_b),
        "Unpushed bookmark {} should NOT be in --tracked output:\n{}",
        branch_b,
        output
    );
}

/// Test that --tracked excludes the default branch (main)
#[tokio::test]
async fn test_tracked_excludes_default_branch() {
    let repo = TestRepo::with_gitlab_remote();

    // Create and push a feature bookmark
    let branch = unique_branch("tracked-feature");
    repo.jj(["new", "main"]);
    repo.create_change("feature.txt", "feature", "Feature commit")
        .create_and_push_bookmark(&branch);

    // Submit with --tracked --dry-run
    let output = repo
        .submit(SubmitCommandConfig {
            tracked: true,
            dry_run: true,
            ..Default::default()
        })
        .await;

    // Feature branch should be in output
    assert!(
        output.contains(&branch),
        "Feature bookmark should be submitted:\n{}",
        output
    );

    // main should NOT be in output
    assert!(
        !output.contains("Would create main"),
        "main should NOT be submitted with --tracked:\n{}",
        output
    );
}
