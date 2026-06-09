use crate::{
    error::Result,
    forge::{CreateMergeRequestOptions, Forge as _, azure::PullRequestStatus, github::GitHubForge},
    tests::TestRepo,
};

#[tokio::test]
async fn submit_creates_pr() -> Result<()> {
    let repo = TestRepo::with_azure_remote();

    let branch = repo.bookmark_name("create-pr");
    repo.jj.exec(["new", "main"])?;
    repo.create_change("test.txt", "content", "Test commit")
        .create_and_push_bookmark(&branch);

    repo.run(["submit", &branch]).await;

    let pr = repo
        .forge()
        .find_merge_request_by_source_branch(&branch)
        .await?
        .expect("PR should exist");

    assert_eq!(pr.source_ref_name, format!("refs/heads/{branch}"));
    assert_eq!(pr.target_ref_name, "refs/heads/main");
    assert_eq!(pr.status, PullRequestStatus::Active);

    Ok(())
}

#[tokio::test]
async fn submit_creates_stacked_prs() -> Result<()> {
    let repo = TestRepo::with_azure_remote();

    let branch_a = repo.bookmark_name("stack-a");
    let branch_b = repo.bookmark_name("stack-b");
    let branch_c = repo.bookmark_name("stack-c");

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
            .map(|pr| pr.target_ref_name.clone()),
        Some("refs/heads/main".to_owned())
    );

    assert_eq!(
        repo.forge()
            .find_merge_request_by_source_branch(&branch_b)
            .await?
            .map(|pr| pr.target_ref_name.clone()),
        Some(format!("refs/heads/{branch_a}"))
    );

    assert_eq!(
        repo.forge()
            .find_merge_request_by_source_branch(&branch_c)
            .await?
            .map(|pr| pr.target_ref_name.clone()),
        Some(format!("refs/heads/{branch_b}"))
    );

    Ok(())
}

#[tokio::test]
async fn submit_is_idempotent() -> Result<()> {
    let repo = TestRepo::with_azure_remote();

    let branch = repo.bookmark_name("idempotent");
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

    assert_eq!(pr1.pull_request_id, pr2.pull_request_id);

    Ok(())
}

#[tokio::test]
async fn submit_retargets_after_middle_bookmark_deleted() -> Result<()> {
    let repo = TestRepo::with_azure_remote();

    let branch_a = repo.bookmark_name("retarget-a");
    let branch_b = repo.bookmark_name("retarget-b");
    let branch_c = repo.bookmark_name("retarget-c");

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
            .map(|pr| pr.target_ref_name.clone()),
        Some(format!("refs/heads/{branch_b}"))
    );

    repo.jj.exec(["bookmark", "delete", &branch_b])?;

    repo.run(["submit", &branch_c]).await;

    assert_eq!(
        repo.forge()
            .find_merge_request_by_source_branch(&branch_c)
            .await?
            .map(|pr| pr.target_ref_name.clone()),
        Some(format!("refs/heads/{branch_a}"))
    );

    Ok(())
}

#[tokio::test]
async fn invalid_token_errors_clearly() -> Result<()> {
    dotenv::dotenv().unwrap();

    let host = std::env::var("GITHUB_HOST").unwrap_or_else(|_| "https://api.github.com".to_owned());
    let project = std::env::var("GITHUB_PROJECT").expect("GITHUB_PROJECT required");
    let ca_bundle = std::env::var("GITHUB_CA_BUNDLE").ok();
    let accept_non_compliant = std::env::var("GITHUB_TLS_ACCEPT_NON_COMPLIANT_CERTS")
        .ok()
        .and_then(|v| v.parse::<bool>().ok())
        .unwrap_or(false);

    let client = GitHubForge::new(
        host,
        project.clone(),
        project,
        "ghp_invalid_token_12345".to_owned(),
        ca_bundle,
        accept_non_compliant,
    )?;

    let result = client
        .create_merge_request(CreateMergeRequestOptions {
            source_branch: TestRepo::new().bookmark_name("invalid-token"),
            target_branch: "main".to_owned(),
            title: "This should fail".to_owned(),
            description: Some("Testing invalid token".to_owned()),
            ..Default::default()
        })
        .await;

    assert!(result.unwrap_err().to_string().contains("401"));

    Ok(())
}

#[tokio::test]
async fn nonexistent_project_errors_clearly() -> Result<()> {
    dotenv::dotenv().unwrap();

    let host = std::env::var("GITHUB_HOST").unwrap_or_else(|_| "https://api.github.com".to_owned());
    let token = std::env::var("GITHUB_TOKEN").expect("GITHUB_TOKEN required");
    let ca_bundle = std::env::var("GITHUB_CA_BUNDLE").ok();
    let accept_non_compliant = std::env::var("GITHUB_TLS_ACCEPT_NON_COMPLIANT_CERTS")
        .ok()
        .and_then(|v| v.parse::<bool>().ok())
        .unwrap_or(false);

    let client = GitHubForge::new(
        host,
        "nonexistent/fake-project-12345".to_owned(),
        "nonexistent/fake-project-12345".to_owned(),
        token,
        ca_bundle,
        accept_non_compliant,
    )?;

    let result = client
        .create_merge_request(CreateMergeRequestOptions {
            source_branch: TestRepo::new().bookmark_name("nonexistent-project"),
            target_branch: "main".to_owned(),
            title: "This should fail".to_owned(),
            description: Some("Testing nonexistent project".to_owned()),
            ..Default::default()
        })
        .await;

    assert!(result.unwrap_err().to_string().contains("404"));

    Ok(())
}
