use crate::{
    error::Result,
    forge::{Forge, ForgeCreateMergeRequestOptions, ForgeMergeRequestState, forgejo::ForgejoForge},
    tests::{TestRepo, unique_branch},
};

#[tokio::test]
async fn test_submit_creates_pr() -> Result<()> {
    let repo = TestRepo::with_forgejo_remote();

    let branch = unique_branch("create-pr");
    repo.jj.exec(["new", "main"])?;
    repo.create_change("test.txt", "content", "Test commit")
        .create_and_push_bookmark(&branch);

    repo.run(["submit", &branch]).await;

    let pr = repo
        .forge()
        .find_merge_request_by_source_branch(&branch)
        .await?
        .expect("PR should exist");

    assert_eq!(pr.source_branch(), branch);
    assert_eq!(pr.target_branch(), "main");
    assert_eq!(pr.state(), ForgeMergeRequestState::Open);

    Ok(())
}

#[tokio::test]
async fn test_submit_creates_stacked_prs() -> Result<()> {
    let repo = TestRepo::with_forgejo_remote();

    let branch_a = unique_branch("stack-a");
    let branch_b = unique_branch("stack-b");
    let branch_c = unique_branch("stack-c");

    // main -> A -> B -> C
    repo.jj.exec(["new", "main"])?;
    repo.create_change("a.txt", "a", "Commit A")
        .create_and_push_bookmark(&branch_a);

    repo.jj.exec(["new"])?;
    repo.create_change("b.txt", "b", "Commit B")
        .create_and_push_bookmark(&branch_b);

    repo.jj.exec(["new"])?;
    repo.create_change("c.txt", "c", "Commit C")
        .create_and_push_bookmark(&branch_c);

    repo.run(["submit", &branch_c]).await;

    assert_eq!(
        repo.forge()
            .find_merge_request_by_source_branch(&branch_a)
            .await?
            .map(|pr| pr.target_branch().to_string()),
        Some("main".to_string())
    );

    assert_eq!(
        repo.forge()
            .find_merge_request_by_source_branch(&branch_b)
            .await?
            .map(|pr| pr.target_branch().to_string()),
        Some(branch_a.to_string())
    );

    assert_eq!(
        repo.forge()
            .find_merge_request_by_source_branch(&branch_c)
            .await?
            .map(|pr| pr.target_branch().to_string()),
        Some(branch_b.to_string())
    );

    Ok(())
}

#[tokio::test]
async fn test_submit_is_idempotent() -> Result<()> {
    let repo = TestRepo::with_forgejo_remote();

    let branch = unique_branch("idempotent");
    repo.jj.exec(["new", "main"])?;
    repo.create_change("test.txt", "content", "Test commit")
        .create_and_push_bookmark(&branch);

    repo.run(["submit", &branch]).await;

    let pr1 = repo
        .forge()
        .find_merge_request_by_source_branch(&branch)
        .await?
        .expect("PR should exist");

    repo.run(["submit", &branch]).await;

    let pr2 = repo
        .forge()
        .find_merge_request_by_source_branch(&branch)
        .await?
        .expect("PR should exist");

    assert_eq!(pr1.iid(), pr2.iid());

    Ok(())
}

#[tokio::test]
async fn test_submit_retargets_after_middle_bookmark_deleted() -> Result<()> {
    let repo = TestRepo::with_forgejo_remote();

    let branch_a = unique_branch("retarget-a");
    let branch_b = unique_branch("retarget-b");
    let branch_c = unique_branch("retarget-c");

    // main -> A -> B -> C
    repo.jj.exec(["new", "main"])?;
    repo.create_change("a.txt", "a", "Commit A")
        .create_and_push_bookmark(&branch_a);

    repo.jj.exec(["new"])?;
    repo.create_change("b.txt", "b", "Commit B")
        .create_and_push_bookmark(&branch_b);

    repo.jj.exec(["new"])?;
    repo.create_change("c.txt", "c", "Commit C")
        .create_and_push_bookmark(&branch_c);

    repo.run(["submit", &branch_c]).await;

    assert_eq!(
        repo.forge()
            .find_merge_request_by_source_branch(&branch_c)
            .await?
            .map(|pr| pr.target_branch().to_string()),
        Some(branch_b.to_string())
    );

    repo.jj.exec(["bookmark", "delete", &branch_b])?;

    repo.run(["submit", &branch_c]).await;

    assert_eq!(
        repo.forge()
            .find_merge_request_by_source_branch(&branch_c)
            .await?
            .map(|pr| pr.target_branch().to_string()),
        Some(branch_a.to_string())
    );

    Ok(())
}

#[tokio::test]
async fn test_invalid_token_errors_clearly() -> Result<()> {
    dotenv::dotenv().ok();

    let host = std::env::var("FORGEJO_HOST").expect("FORGEJO_HOST required");
    let project = std::env::var("FORGEJO_PROJECT").expect("FORGEJO_PROJECT required");
    let ca_bundle = std::env::var("FORGEJO_CA_BUNDLE").ok();
    let accept_non_compliant = std::env::var("FORGEJO_TLS_ACCEPT_NON_COMPLIANT_CERTS")
        .ok()
        .and_then(|v| v.parse::<bool>().ok())
        .unwrap_or(false);

    let client = ForgejoForge::new(
        host,
        project.clone(),
        project,
        "invalid-token-12345".to_string(),
        ca_bundle,
        accept_non_compliant,
    )?;

    let result = client
        .create_merge_request(
            ForgeCreateMergeRequestOptions::builder()
                .source_branch(unique_branch("invalid-token"))
                .target_branch("main".to_string())
                .title("This should fail".to_string())
                .description("Testing invalid token".to_string())
                .build(),
        )
        .await;

    assert!(result.unwrap_err().to_string().contains("401"));

    Ok(())
}

#[tokio::test]
async fn test_nonexistent_project_errors_clearly() -> Result<()> {
    dotenv::dotenv().ok();

    let host = std::env::var("FORGEJO_HOST").expect("FORGEJO_HOST required");
    let token = std::env::var("FORGEJO_TOKEN").expect("FORGEJO_TOKEN required");
    let ca_bundle = std::env::var("FORGEJO_CA_BUNDLE").ok();
    let accept_non_compliant = std::env::var("FORGEJO_TLS_ACCEPT_NON_COMPLIANT_CERTS")
        .ok()
        .and_then(|v| v.parse::<bool>().ok())
        .unwrap_or(false);

    let client = ForgejoForge::new(
        host,
        "nonexistent/fake-project-12345",
        "nonexistent/fake-project-12345",
        token,
        ca_bundle,
        accept_non_compliant,
    )?;

    let result = client
        .create_merge_request(
            ForgeCreateMergeRequestOptions::builder()
                .source_branch(unique_branch("nonexistent-project"))
                .target_branch("main".to_string())
                .title("This should fail".to_string())
                .description("Testing nonexistent project".to_string())
                .build(),
        )
        .await;

    assert!(result.unwrap_err().to_string().contains("404"));

    Ok(())
}
