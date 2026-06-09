use crate::{
    error::Result,
    forge::{CreateMergeRequestOptions, Forge as _, forgejo::ForgejoForge},
    tests::TestRepo,
};

#[tokio::test]
async fn submit_creates_pr() -> Result<()> {
    let repo = TestRepo::with_forgejo_remote();

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

    assert_eq!(pr.pull_request.head.ref_name, branch);
    assert_eq!(pr.pull_request.base.ref_name, "main");
    assert_eq!(pr.pull_request.state, "open");

    Ok(())
}

#[tokio::test]
async fn submit_creates_stacked_prs() -> Result<()> {
    let repo = TestRepo::with_forgejo_remote();

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
            .map(|pr| pr.pull_request.base.ref_name.clone()),
        Some("main".to_owned())
    );

    assert_eq!(
        repo.forge()
            .find_merge_request_by_source_branch(&branch_b)
            .await?
            .map(|pr| pr.pull_request.base.ref_name.clone()),
        Some(branch_a.clone())
    );

    assert_eq!(
        repo.forge()
            .find_merge_request_by_source_branch(&branch_c)
            .await?
            .map(|pr| pr.pull_request.base.ref_name.clone()),
        Some(branch_b.clone())
    );

    Ok(())
}

#[tokio::test]
async fn submit_is_idempotent() -> Result<()> {
    let repo = TestRepo::with_forgejo_remote();

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

    assert_eq!(pr1.pull_request.number, pr2.pull_request.number);

    Ok(())
}

#[tokio::test]
async fn submit_retargets_after_middle_bookmark_deleted() -> Result<()> {
    let repo = TestRepo::with_forgejo_remote();

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
            .map(|pr| pr.pull_request.base.ref_name.clone()),
        Some(branch_b.clone())
    );

    repo.jj.exec(["bookmark", "delete", &branch_b])?;

    repo.run(["submit", &branch_c]).await;

    assert_eq!(
        repo.forge()
            .find_merge_request_by_source_branch(&branch_c)
            .await?
            .map(|pr| pr.pull_request.base.ref_name.clone()),
        Some(branch_a.clone())
    );

    Ok(())
}

#[tokio::test]
async fn invalid_token_errors_clearly() -> Result<()> {
    dotenv::dotenv().unwrap();

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
        "invalid-token-12345".to_owned(),
        ca_bundle,
        accept_non_compliant,
        "WIP: ".to_owned(),
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
        "WIP: ".to_owned(),
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
