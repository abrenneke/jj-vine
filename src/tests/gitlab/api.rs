use assertables::assert_contains;

use crate::{
    error::Result,
    forge::{Forge, ForgeCreateMergeRequestOptions, gitlab::GitLabForge},
    tests::{TestRepo, unique_branch},
};

#[tokio::test]
async fn test_submit_creates_mr() -> Result<()> {
    let repo = TestRepo::with_gitlab_remote();

    let branch = unique_branch("create-mr");
    repo.jj.exec(["new", "main"]).unwrap();
    repo.create_change("test.txt", "content", "Test commit")
        .create_and_push_bookmark(&branch);

    repo.run(["submit", &branch]).await;

    let mr = repo
        .forge()
        .find_merge_request_by_source_branch(&branch)
        .await?
        .unwrap_or_else(|| panic!("MR should exist"));

    assert_eq!(mr.source_branch, branch);
    assert_eq!(mr.target_branch, "main");
    assert_eq!(&mr.state, "opened");

    Ok(())
}

#[tokio::test]
async fn test_submit_creates_stacked_mrs() -> Result<()> {
    let repo = TestRepo::with_gitlab_remote();

    let branch_a = unique_branch("stack-a");
    let branch_b = unique_branch("stack-b");
    let branch_c = unique_branch("stack-c");

    // main -> A -> B -> C
    repo.jj.exec(["new", "main"]).unwrap();
    repo.create_change("a.txt", "a", "Commit A")
        .create_and_push_bookmark(&branch_a);

    repo.jj.exec(["new"]).unwrap();
    repo.create_change("b.txt", "b", "Commit B")
        .create_and_push_bookmark(&branch_b);

    repo.jj.exec(["new"]).unwrap();
    repo.create_change("c.txt", "c", "Commit C")
        .create_and_push_bookmark(&branch_c);

    repo.run(["submit", &branch_c]).await;

    assert_eq!(
        repo.forge()
            .find_merge_request_by_source_branch(&branch_a)
            .await?
            .map(|mr| mr.target_branch.to_string()),
        Some("main".to_string())
    );

    assert_eq!(
        repo.forge()
            .find_merge_request_by_source_branch(&branch_b)
            .await?
            .map(|mr| mr.target_branch.to_string()),
        Some(branch_a)
    );

    assert_eq!(
        repo.forge()
            .find_merge_request_by_source_branch(&branch_c)
            .await?
            .map(|mr| mr.target_branch.to_string()),
        Some(branch_b)
    );

    Ok(())
}

#[tokio::test]
async fn test_submit_is_idempotent() -> Result<()> {
    let repo = TestRepo::with_gitlab_remote();

    let branch = unique_branch("idempotent");
    repo.jj.exec(["new", "main"]).unwrap();
    repo.create_change("test.txt", "content", "Test commit")
        .create_and_push_bookmark(&branch);

    repo.run(["submit", &branch]).await;

    let mr1 = repo
        .forge()
        .find_merge_request_by_source_branch(&branch)
        .await?
        .expect("MR should exist");

    repo.run(["submit", &branch]).await;

    let mr2 = repo
        .forge()
        .find_merge_request_by_source_branch(&branch)
        .await?
        .expect("MR should exist");

    assert_eq!(mr1.iid, mr2.iid);

    Ok(())
}

#[tokio::test]
async fn test_submit_retargets_after_middle_bookmark_deleted() -> Result<()> {
    let repo = TestRepo::with_gitlab_remote();

    let branch_a = unique_branch("retarget-a");
    let branch_b = unique_branch("retarget-b");
    let branch_c = unique_branch("retarget-c");

    // Create stack: main -> A -> B -> C
    repo.jj.exec(["new", "main"]).unwrap();
    repo.create_change("a.txt", "a", "Commit A")
        .create_and_push_bookmark(&branch_a);

    repo.jj.exec(["new"]).unwrap();
    repo.create_change("b.txt", "b", "Commit B")
        .create_and_push_bookmark(&branch_b);

    repo.jj.exec(["new"]).unwrap();
    repo.create_change("c.txt", "c", "Commit C")
        .create_and_push_bookmark(&branch_c);

    repo.run(["submit", &branch_c]).await;

    let mr_c = repo
        .forge()
        .find_merge_request_by_source_branch(&branch_c)
        .await?
        .unwrap();
    assert_eq!(mr_c.target_branch, branch_b);

    repo.jj.exec(["bookmark", "delete", &branch_b])?;

    repo.run(["submit", &branch_c]).await;

    let mr_c_updated = repo
        .forge()
        .find_merge_request_by_source_branch(&branch_c)
        .await?
        .unwrap();

    assert_eq!(mr_c_updated.target_branch, branch_a);

    Ok(())
}

#[tokio::test]
async fn test_invalid_token_errors_clearly() -> Result<()> {
    dotenv::dotenv().ok();

    let host = std::env::var("GITLAB_HOST").expect("GITLAB_HOST required");
    let project = std::env::var("GITLAB_PROJECT").expect("GITLAB_PROJECT required");
    let ca_bundle = std::env::var("GITLAB_CA_BUNDLE").ok();
    let accept_non_compliant = std::env::var("GITLAB_TLS_ACCEPT_NON_COMPLIANT_CERTS")
        .ok()
        .and_then(|v| v.parse::<bool>().ok())
        .unwrap_or(false);

    let client = GitLabForge::new(
        host,
        project.clone(),
        project,
        "invalid-token-12345".to_string(),
        ca_bundle,
        accept_non_compliant,
        true,
    )?;

    let result = client
        .create_merge_request(ForgeCreateMergeRequestOptions {
            source_branch: unique_branch("invalid-token"),
            target_branch: "main".to_string(),
            title: "This should fail".to_string(),
            description: Some("Testing invalid token".to_string()),
            remove_source_branch: true,
            ..Default::default()
        })
        .await;

    assert!(result.unwrap_err().to_string().contains("401"));

    Ok(())
}

#[tokio::test]
async fn test_nonexistent_project_errors_clearly() -> Result<()> {
    dotenv::dotenv().ok();

    let host = std::env::var("GITLAB_HOST").expect("GITLAB_HOST required");
    let token = std::env::var("GITLAB_TOKEN").expect("GITLAB_TOKEN required");
    let ca_bundle = std::env::var("GITLAB_CA_BUNDLE").ok();
    let accept_non_compliant = std::env::var("GITLAB_TLS_ACCEPT_NON_COMPLIANT_CERTS")
        .ok()
        .and_then(|v| v.parse::<bool>().ok())
        .unwrap_or(false);

    // Create client with nonexistent project
    let client = GitLabForge::new(
        host,
        "nonexistent/fake-project-12345".to_string(),
        "nonexistent/fake-project-12345".to_string(),
        token,
        ca_bundle,
        accept_non_compliant,
        true,
    )?;

    let result = client
        .create_merge_request(ForgeCreateMergeRequestOptions {
            source_branch: unique_branch("nonexistent-project"),
            target_branch: "main".to_string(),
            title: "This should fail".to_string(),
            description: Some("Testing nonexistent project".to_string()),
            remove_source_branch: true,
            ..Default::default()
        })
        .await;

    assert!(result.unwrap_err().to_string().contains("404"));

    Ok(())
}

#[tokio::test]
async fn test_create_merge_request_dependencies() -> Result<()> {
    let repo = TestRepo::with_gitlab_remote();

    let a = unique_branch("dependency-a");
    let b = unique_branch("dependency-b");
    let c = unique_branch("dependency-c");

    // main -> A -> C
    // main -> B -> C

    repo.new_on("main")
        .create_change_and_tracked_bookmark(&a)
        .new_on("main")
        .create_change_and_tracked_bookmark(&b)
        .exec(["new", &a, &b])
        .create_change_and_tracked_bookmark(&c);

    repo.run(["submit", &c]).await;

    let mr_a = repo
        .forge()
        .find_merge_request_by_source_branch(&a)
        .await?
        .expect("MR A should exist");

    let mr_b = repo
        .forge()
        .find_merge_request_by_source_branch(&b)
        .await?
        .expect("MR B should exist");

    let mr_c = repo
        .forge()
        .find_merge_request_by_source_branch(&c)
        .await?
        .expect("MR C should exist");

    let deps: Vec<_> = repo
        .forge()
        .get_merge_request_dependencies(mr_c.iid)
        .await?
        .iter()
        .map(|dep| dep.blocking_merge_request.iid)
        .collect();

    assert_eq!(deps.len(), 2);
    assert_contains!(deps, &mr_a.iid);
    assert_contains!(deps, &mr_b.iid);

    Ok(())
}
