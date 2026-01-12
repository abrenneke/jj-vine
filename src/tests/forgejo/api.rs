use crate::{
    commands::submit::SubmitCommandConfig,
    forge::{Forge, ForgeCreateMergeRequestOptions, ForgeMergeRequestState, forgejo::ForgejoForge},
    tests::{TestRepo, unique_branch},
};

/// Test that submit creates a PR via the Forgejo API
#[tokio::test]
async fn test_submit_creates_pr() {
    let repo = TestRepo::with_forgejo_remote();

    let branch = unique_branch("create-pr");
    repo.jj(["new", "main"]);
    repo.create_change("test.txt", "content", "Test commit")
        .create_and_push_bookmark(&branch);

    // Submit the bookmark
    repo.submit(SubmitCommandConfig {
        bookmark: Some(branch.clone()),
        ..Default::default()
    })
    .await;

    // Verify PR was created
    let pr = repo
        .forge()
        .find_merge_request_by_source_branch(&branch)
        .await
        .expect("Failed to query Forgejo")
        .expect("PR should exist");

    assert_eq!(pr.source_branch(), branch);
    assert_eq!(pr.target_branch(), "main");
    assert_eq!(pr.state(), ForgeMergeRequestState::Open);
}

/// Test that submit creates stacked PRs with correct targets
#[tokio::test]
async fn test_submit_creates_stacked_prs() {
    let repo = TestRepo::with_forgejo_remote();

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

    // Verify PR A: targets main
    let pr_a = repo
        .forge()
        .find_merge_request_by_source_branch(&branch_a)
        .await
        .expect("Failed to query Forgejo")
        .expect("PR A should exist");
    assert_eq!(pr_a.target_branch(), "main");

    // Verify PR B: targets A
    let pr_b = repo
        .forge()
        .find_merge_request_by_source_branch(&branch_b)
        .await
        .expect("Failed to query Forgejo")
        .expect("PR B should exist");
    assert_eq!(pr_b.target_branch(), branch_a);

    // Verify PR C: targets B
    let pr_c = repo
        .forge()
        .find_merge_request_by_source_branch(&branch_c)
        .await
        .expect("Failed to query Forgejo")
        .expect("PR C should exist");
    assert_eq!(pr_c.target_branch(), branch_b);
}

/// Test that resubmitting finds and reuses existing PR
#[tokio::test]
async fn test_submit_is_idempotent() {
    let repo = TestRepo::with_forgejo_remote();

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

    let pr1 = repo
        .forge()
        .find_merge_request_by_source_branch(&branch)
        .await
        .expect("Failed to query")
        .expect("PR should exist");

    // Second submit (should reuse existing PR)
    repo.submit(SubmitCommandConfig {
        bookmark: Some(branch.clone()),
        ..Default::default()
    })
    .await;

    let pr2 = repo
        .forge()
        .find_merge_request_by_source_branch(&branch)
        .await
        .expect("Failed to query")
        .expect("PR should exist");

    // Same PR should be reused
    assert_eq!(pr1.iid(), pr2.iid(), "Should reuse the same PR");
}

/// Test that PR is retargeted when middle bookmark is deleted
#[tokio::test]
async fn test_submit_retargets_after_middle_bookmark_deleted() {
    let repo = TestRepo::with_forgejo_remote();

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
    let pr_c = repo
        .forge()
        .find_merge_request_by_source_branch(&branch_c)
        .await
        .expect("Query failed")
        .expect("PR C should exist");
    assert_eq!(pr_c.target_branch(), branch_b);

    // Delete bookmark B
    repo.jj(["bookmark", "delete", &branch_b]);

    // Resubmit C - should retarget to A
    repo.submit(SubmitCommandConfig {
        bookmark: Some(branch_c.clone()),
        ..Default::default()
    })
    .await;

    // Verify C now targets A
    let pr_c_updated = repo
        .forge()
        .find_merge_request_by_source_branch(&branch_c)
        .await
        .expect("Query failed")
        .expect("PR C should exist");
    assert_eq!(
        pr_c_updated.target_branch(),
        branch_a,
        "PR C should now target A after B was deleted"
    );
}

/// Test that invalid token produces clear 401 error
#[tokio::test]
async fn test_invalid_token_errors_clearly() {
    dotenv::dotenv().ok();

    let host = std::env::var("FORGEJO_HOST").expect("FORGEJO_HOST required");
    let project = std::env::var("FORGEJO_PROJECT").expect("FORGEJO_PROJECT required");
    let ca_bundle = std::env::var("FORGEJO_CA_BUNDLE").ok();
    let accept_non_compliant = std::env::var("FORGEJO_TLS_ACCEPT_NON_COMPLIANT_CERTS")
        .ok()
        .and_then(|v| v.parse::<bool>().ok())
        .unwrap_or(false);

    // Create client with invalid token
    let client = ForgejoForge::new(
        host,
        project,
        "invalid-token-12345".to_string(),
        ca_bundle,
        accept_non_compliant,
    )
    .expect("Failed to create Forgejo client");

    let branch_name = unique_branch("invalid-token");

    // Attempt to create PR with invalid token
    let result = client
        .create_merge_request(
            ForgeCreateMergeRequestOptions::builder()
                .source_branch(branch_name.clone())
                .target_branch("main".to_string())
                .title("This should fail".to_string())
                .description("Testing invalid token".to_string())
                .build(),
        )
        .await;

    assert!(result.is_err(), "Should fail with invalid token");
    let err = result.unwrap_err().to_string();

    assert!(
        err.contains("401"),
        "Error should mention invalid token: {}",
        err
    );
}

/// Test that nonexistent project produces clear 404 error
#[tokio::test]
async fn test_nonexistent_project_errors_clearly() {
    dotenv::dotenv().ok();

    let host = std::env::var("FORGEJO_HOST").expect("FORGEJO_HOST required");
    let token = std::env::var("FORGEJO_TOKEN").expect("FORGEJO_TOKEN required");
    let ca_bundle = std::env::var("FORGEJO_CA_BUNDLE").ok();
    let accept_non_compliant = std::env::var("FORGEJO_TLS_ACCEPT_NON_COMPLIANT_CERTS")
        .ok()
        .and_then(|v| v.parse::<bool>().ok())
        .unwrap_or(false);

    // Create client with nonexistent project
    let client = ForgejoForge::new(
        host,
        "nonexistent/fake-project-12345".to_string(),
        token,
        ca_bundle,
        accept_non_compliant,
    )
    .expect("Failed to create Forgejo client");

    let branch_name = unique_branch("nonexistent-project");

    // Attempt to create PR with nonexistent project
    let result = client
        .create_merge_request(
            ForgeCreateMergeRequestOptions::builder()
                .source_branch(branch_name.clone())
                .target_branch("main".to_string())
                .title("This should fail".to_string())
                .description("Testing nonexistent project".to_string())
                .build(),
        )
        .await;

    assert!(result.is_err(), "Should fail with nonexistent project");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("404") || err.to_lowercase().contains("not found"),
        "Error should mention project not found: {}",
        err
    );
}
