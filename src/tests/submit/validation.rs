use std::process::Command;

use crate::{commands::submit::SubmitCommandConfig, tests::TestRepo};

/// Test that specifying both --bookmark and --tracked is an error
#[tokio::test]
async fn test_bookmark_and_tracked_mutually_exclusive() {
    let repo = TestRepo::with_gitlab_remote();

    let result = repo
        .try_submit(SubmitCommandConfig {
            bookmark: Some("some-bookmark".to_string()),
            tracked: true,
            ..Default::default()
        })
        .await;

    assert!(
        result.is_err(),
        "Should error when both bookmark and tracked are specified"
    );
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("Cannot specify both") || err.contains("mutually exclusive"),
        "Error should mention mutual exclusivity: {}",
        err
    );
}

/// Test that submit requires either --bookmark or --tracked
#[tokio::test]
async fn test_submit_requires_bookmark_or_tracked() {
    let repo = TestRepo::with_gitlab_remote();

    let result = repo
        .try_submit(SubmitCommandConfig {
            bookmark: None,
            tracked: false,
            ..Default::default()
        })
        .await;

    assert!(
        result.is_err(),
        "Should error when neither bookmark nor tracked is specified"
    );
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("Must specify") || err.contains("required"),
        "Error should mention requirement: {}",
        err
    );
}

/// Test that --tracked with no pushed bookmarks gives a clear error
#[tokio::test]
async fn test_tracked_with_no_pushed_bookmarks() {
    let repo = TestRepo::with_gitlab_remote();

    // Create a bookmark but don't push it
    repo.jj(["new", "main"]);
    repo.create_change("test.txt", "content", "Test commit")
        .create_bookmark("unpushed-branch");

    let result = repo
        .try_submit(SubmitCommandConfig {
            tracked: true,
            ..Default::default()
        })
        .await;

    assert!(
        result.is_err(),
        "Should error when no tracked bookmarks exist"
    );
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("No tracked bookmarks") || err.contains("no bookmarks"),
        "Error should mention no tracked bookmarks: {}",
        err
    );
}

/// Test that push failure prevents MR creation
#[tokio::test]
async fn test_push_failure_skips_mr_creation() {
    dotenv::dotenv().ok();

    let host = std::env::var("GITLAB_HOST").expect("GITLAB_HOST required");
    let project = std::env::var("GITLAB_PROJECT").expect("GITLAB_PROJECT required");
    let token = std::env::var("GITLAB_TOKEN").expect("GITLAB_TOKEN required");

    let repo = TestRepo::new();

    // Create a local bare repo to use as origin
    let temp_init = repo.path.join("temp_init");
    let remote_dir = repo.path.join("remote.git");
    std::fs::create_dir(&temp_init).unwrap();

    // Initialize temp repo with main branch
    Command::new("git")
        .args(["init"])
        .current_dir(&temp_init)
        .output()
        .unwrap();
    std::fs::write(temp_init.join("README.md"), "# Test").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(&temp_init)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "Initial commit"])
        .current_dir(&temp_init)
        .output()
        .unwrap();

    // Clone to bare repo
    Command::new("git")
        .args([
            "clone",
            "--bare",
            temp_init.to_str().unwrap(),
            remote_dir.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    // Add local bare repo as origin (so push goes there, not GitLab)
    repo.jj([
        "git",
        "remote",
        "add",
        "origin",
        remote_dir.to_str().unwrap(),
    ]);

    // Fetch and track main
    repo.jj(["git", "fetch"]);
    repo.jj(["bookmark", "track", "main@origin"]);

    // Configure jj-vine to point to GitLab (for MR creation attempt)
    repo.jj(["config", "set", "--repo", "jj-vine.gitlabHost", &host]);
    repo.jj(["config", "set", "--repo", "jj-vine.gitlabProject", &project]);
    repo.jj(["config", "set", "--repo", "jj-vine.gitlabToken", &token]);

    // Create feature bookmark
    repo.jj(["new", "main"]);
    repo.create_change("test.txt", "content", "Feature commit")
        .create_bookmark("feature-push-fail");

    // Remove the bare repo to cause push failure
    std::fs::remove_dir_all(&remote_dir).unwrap();

    // Try to submit - should fail on push
    let result = repo
        .try_submit(SubmitCommandConfig {
            bookmark: Some("feature-push-fail".to_string()),
            ..Default::default()
        })
        .await;

    assert!(result.is_err(), "Should fail when push fails");
    let err = result.unwrap_err().to_string();
    assert!(
        err.to_lowercase().contains("push")
            || err.to_lowercase().contains("remote")
            || err.to_lowercase().contains("failed"),
        "Error should mention push failure: {}",
        err
    );
}
