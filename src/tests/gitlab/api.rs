use crate::{
    commands::submit::SubmitCommandConfig,
    gitlab::GitLabClient,
    tests::{TestRepo, unique_branch},
};

/// Test that submit creates an MR via the GitLab API
#[tokio::test]
async fn test_submit_creates_mr() {
    let repo = TestRepo::with_gitlab_remote();

    let branch = unique_branch("create-mr");
    repo.jj(["new", "main"]);
    repo.create_change("test.txt", "content", "Test commit")
        .create_and_push_bookmark(&branch);

    // Submit the bookmark
    repo.submit(SubmitCommandConfig {
        bookmark: Some(branch.clone()),
        ..Default::default()
    })
    .await;

    // Verify MR was created
    let mr = repo
        .gitlab()
        .find_mr_by_source_branch(&branch)
        .await
        .expect("Failed to query GitLab")
        .expect("MR should exist");

    assert_eq!(mr.source_branch, branch);
    assert_eq!(mr.target_branch, "main");
    assert_eq!(mr.state, "opened");
}

/// Test that submit creates stacked MRs with correct targets
#[tokio::test]
async fn test_submit_creates_stacked_mrs() {
    let repo = TestRepo::with_gitlab_remote();

    let branch_a = unique_branch("stack-a");
    let branch_b = unique_branch("stack-b");
    let branch_c = unique_branch("stack-c");

    // Create stack: main -> A -> B -> C
    repo.jj(["new", "main"]);
    repo.create_change("a.txt", "a", "Commit A")
        .create_and_push_bookmark(&branch_a);

    repo.jj(["new"]);
    repo.create_change("b.txt", "b", "Commit B")
        .create_and_push_bookmark(&branch_b);

    repo.jj(["new"]);
    repo.create_change("c.txt", "c", "Commit C")
        .create_and_push_bookmark(&branch_c);

    // Submit the entire stack via C
    repo.submit(SubmitCommandConfig {
        bookmark: Some(branch_c.clone()),
        ..Default::default()
    })
    .await;

    // Verify MR A: targets main
    let mr_a = repo
        .gitlab()
        .find_mr_by_source_branch(&branch_a)
        .await
        .expect("Failed to query GitLab")
        .expect("MR A should exist");
    assert_eq!(mr_a.target_branch, "main");

    // Verify MR B: targets A
    let mr_b = repo
        .gitlab()
        .find_mr_by_source_branch(&branch_b)
        .await
        .expect("Failed to query GitLab")
        .expect("MR B should exist");
    assert_eq!(mr_b.target_branch, branch_a);

    // Verify MR C: targets B
    let mr_c = repo
        .gitlab()
        .find_mr_by_source_branch(&branch_c)
        .await
        .expect("Failed to query GitLab")
        .expect("MR C should exist");
    assert_eq!(mr_c.target_branch, branch_b);
}

/// Test that resubmitting finds and reuses existing MR
#[tokio::test]
async fn test_submit_is_idempotent() {
    let repo = TestRepo::with_gitlab_remote();

    let branch = unique_branch("idempotent");
    repo.jj(["new", "main"]);
    repo.create_change("test.txt", "content", "Test commit")
        .create_and_push_bookmark(&branch);

    // First submit
    repo.submit(SubmitCommandConfig {
        bookmark: Some(branch.clone()),
        ..Default::default()
    })
    .await;

    let mr1 = repo
        .gitlab()
        .find_mr_by_source_branch(&branch)
        .await
        .expect("Failed to query")
        .expect("MR should exist");

    // Second submit (should reuse existing MR)
    repo.submit(SubmitCommandConfig {
        bookmark: Some(branch.clone()),
        ..Default::default()
    })
    .await;

    let mr2 = repo
        .gitlab()
        .find_mr_by_source_branch(&branch)
        .await
        .expect("Failed to query")
        .expect("MR should exist");

    // Same MR should be reused
    assert_eq!(mr1.iid, mr2.iid, "Should reuse the same MR");
}

/// Test that MR is retargeted when middle bookmark is deleted
#[tokio::test]
async fn test_submit_retargets_after_middle_bookmark_deleted() {
    let repo = TestRepo::with_gitlab_remote();

    let branch_a = unique_branch("retarget-a");
    let branch_b = unique_branch("retarget-b");
    let branch_c = unique_branch("retarget-c");

    // Create stack: main -> A -> B -> C
    repo.jj(["new", "main"]);
    repo.create_change("a.txt", "a", "Commit A")
        .create_and_push_bookmark(&branch_a);

    repo.jj(["new"]);
    repo.create_change("b.txt", "b", "Commit B")
        .create_and_push_bookmark(&branch_b);

    repo.jj(["new"]);
    repo.create_change("c.txt", "c", "Commit C")
        .create_and_push_bookmark(&branch_c);

    // Submit all three
    repo.submit(SubmitCommandConfig {
        bookmark: Some(branch_c.clone()),
        ..Default::default()
    })
    .await;

    // Verify C targets B initially
    let mr_c = repo
        .gitlab()
        .find_mr_by_source_branch(&branch_c)
        .await
        .expect("Query failed")
        .expect("MR C should exist");
    assert_eq!(mr_c.target_branch, branch_b);

    // Delete bookmark B
    repo.jj(["bookmark", "delete", &branch_b]);

    // Resubmit C - should retarget to A
    repo.submit(SubmitCommandConfig {
        bookmark: Some(branch_c.clone()),
        ..Default::default()
    })
    .await;

    // Verify C now targets A
    let mr_c_updated = repo
        .gitlab()
        .find_mr_by_source_branch(&branch_c)
        .await
        .expect("Query failed")
        .expect("MR C should exist");
    assert_eq!(
        mr_c_updated.target_branch, branch_a,
        "MR C should now target A after B was deleted"
    );
}

/// Test that invalid token produces clear 401 error
#[tokio::test]
async fn test_invalid_token_errors_clearly() {
    dotenv::dotenv().ok();

    let host = std::env::var("GITLAB_HOST").expect("GITLAB_HOST required");
    let project = std::env::var("GITLAB_PROJECT").expect("GITLAB_PROJECT required");
    let ca_bundle = std::env::var("GITLAB_CA_BUNDLE").ok();
    let accept_non_compliant = std::env::var("GITLAB_TLS_ACCEPT_NON_COMPLIANT_CERTS")
        .ok()
        .and_then(|v| v.parse::<bool>().ok())
        .unwrap_or(false);

    // Create client with invalid token
    let client = GitLabClient::new(
        host,
        project,
        "invalid-token-12345".to_string(),
        ca_bundle,
        accept_non_compliant,
    )
    .expect("Failed to create GitLab client");

    let branch_name = unique_branch("invalid-token");

    // Attempt to create MR with invalid token
    let result = client
        .create_merge_request()
        .source_branch(&branch_name)
        .target_branch("main")
        .title("This should fail")
        .description("Testing invalid token")
        .remove_source_branch(true)
        .squash(false)
        .call()
        .await;

    assert!(result.is_err(), "Should fail with invalid token");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("401") || err.to_lowercase().contains("unauthorized"),
        "Error should mention authentication issue: {}",
        err
    );
}

/// Test that nonexistent project produces clear 404 error
#[tokio::test]
async fn test_nonexistent_project_errors_clearly() {
    dotenv::dotenv().ok();

    let host = std::env::var("GITLAB_HOST").expect("GITLAB_HOST required");
    let token = std::env::var("GITLAB_TOKEN").expect("GITLAB_TOKEN required");
    let ca_bundle = std::env::var("GITLAB_CA_BUNDLE").ok();
    let accept_non_compliant = std::env::var("GITLAB_TLS_ACCEPT_NON_COMPLIANT_CERTS")
        .ok()
        .and_then(|v| v.parse::<bool>().ok())
        .unwrap_or(false);

    // Create client with nonexistent project
    let client = GitLabClient::new(
        host,
        "nonexistent/fake-project-12345".to_string(),
        token,
        ca_bundle,
        accept_non_compliant,
    )
    .expect("Failed to create GitLab client");

    let branch_name = unique_branch("nonexistent-project");

    // Attempt to create MR with nonexistent project
    let result = client
        .create_merge_request()
        .source_branch(&branch_name)
        .target_branch("main")
        .title("This should fail")
        .description("Testing nonexistent project")
        .remove_source_branch(true)
        .squash(false)
        .call()
        .await;

    assert!(result.is_err(), "Should fail with nonexistent project");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("404") || err.to_lowercase().contains("not found"),
        "Error should mention project not found: {}",
        err
    );
}
