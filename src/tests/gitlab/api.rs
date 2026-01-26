use assertables::assert_contains;
use rstest::rstest;

use crate::{
    config::{Config, ForgeType},
    error::Result,
    forge::{Forge, ForgeCreateMergeRequestOptions, ForgeImpl, ForgeMergeRequestState},
    tests::{TestRepo, unique_branch},
};

#[rstest]
#[case(TestRepo::with_gitlab_remote())]
#[case(TestRepo::with_github_remote())]
#[case(TestRepo::with_forgejo_remote())]
#[case(TestRepo::with_azure_remote())]
#[tokio::test]
async fn test_submit_creates_mr<T: Forge>(#[case] repo: TestRepo<T>) -> Result<()> {
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

    assert_eq!(mr.source_branch(), branch);
    assert_eq!(mr.target_branch(), "main");
    assert_eq!(mr.state(), ForgeMergeRequestState::Open);

    Ok(())
}

#[rstest]
#[case(TestRepo::with_gitlab_remote())]
#[case(TestRepo::with_github_remote())]
#[case(TestRepo::with_forgejo_remote())]
#[case(TestRepo::with_azure_remote())]
#[tokio::test]
async fn test_submit_creates_stacked_mrs<T: Forge>(#[case] repo: TestRepo<T>) -> Result<()> {
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
            .map(|mr| mr.target_branch().to_string()),
        Some("main".to_string())
    );

    assert_eq!(
        repo.forge()
            .find_merge_request_by_source_branch(&branch_b)
            .await?
            .map(|mr| mr.target_branch().to_string()),
        Some(branch_a)
    );

    assert_eq!(
        repo.forge()
            .find_merge_request_by_source_branch(&branch_c)
            .await?
            .map(|mr| mr.target_branch().to_string()),
        Some(branch_b)
    );

    Ok(())
}

#[rstest]
#[case(TestRepo::with_gitlab_remote())]
#[case(TestRepo::with_github_remote())]
#[case(TestRepo::with_forgejo_remote())]
#[case(TestRepo::with_azure_remote())]
#[tokio::test]
async fn test_submit_is_idempotent<T: Forge>(#[case] repo: TestRepo<T>) -> Result<()> {
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

    assert_eq!(mr1.iid(), mr2.iid());

    Ok(())
}

#[rstest]
#[case(TestRepo::with_gitlab_remote())]
#[case(TestRepo::with_github_remote())]
#[case(TestRepo::with_forgejo_remote())]
#[case(TestRepo::with_azure_remote())]
#[tokio::test]
async fn test_submit_retargets_after_middle_bookmark_deleted<T: Forge>(
    #[case] repo: TestRepo<T>,
) -> Result<()> {
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
        .expect("MR C should exist");
    assert_eq!(mr_c.target_branch(), branch_b);

    repo.jj.exec(["bookmark", "delete", &branch_b])?;

    repo.run(["submit", &branch_c]).await;

    let mr_c_updated = repo
        .forge()
        .find_merge_request_by_source_branch(&branch_c)
        .await?
        .expect("MR C should exist");

    assert_eq!(
        mr_c_updated.target_branch(),
        branch_a,
        "MR C should now target A after B was deleted"
    );

    Ok(())
}

#[rstest]
#[case(ForgeType::GitLab)]
#[case(ForgeType::GitHub)]
#[case(ForgeType::Forgejo)]
#[case(ForgeType::AzureDevOps)]
#[tokio::test]
async fn test_invalid_token_errors_clearly(#[case] forge_type: ForgeType) -> Result<()> {
    dotenv::dotenv().ok();

    let config = Config::builder().forge(forge_type);
    let config = match forge_type {
        ForgeType::GitLab => {
            let host = std::env::var("GITLAB_HOST").expect("GITLAB_HOST required");
            let project = std::env::var("GITLAB_PROJECT").expect("GITLAB_PROJECT required");
            let ca_bundle = std::env::var("GITLAB_CA_BUNDLE").ok();
            let accept_non_compliant = std::env::var("GITLAB_TLS_ACCEPT_NON_COMPLIANT_CERTS")
                .ok()
                .and_then(|v| v.parse::<bool>().ok())
                .unwrap_or(false);
            config
                .tls_accept_non_compliant_certs(accept_non_compliant)
                .maybe_ca_bundle(ca_bundle)
                .gitlab(crate::config::GitLabConfig {
                    host: host.clone(),
                    project: project.clone(),
                    token: "invalid-token-12345".to_string(),
                    target_project: project.clone(),
                    create_merge_request_dependencies: true,
                })
                .build()
        }
        ForgeType::GitHub => {
            let host = std::env::var("GITHUB_HOST")
                .unwrap_or_else(|_| "https://api.github.com".to_string());
            let project = std::env::var("GITHUB_PROJECT").expect("GITHUB_PROJECT required");
            let ca_bundle = std::env::var("GITHUB_CA_BUNDLE").ok();
            let accept_non_compliant = std::env::var("GITHUB_TLS_ACCEPT_NON_COMPLIANT_CERTS")
                .ok()
                .and_then(|v| v.parse::<bool>().ok())
                .unwrap_or(false);

            config
                .tls_accept_non_compliant_certs(accept_non_compliant)
                .maybe_ca_bundle(ca_bundle)
                .github(crate::config::GitHubConfig {
                    host: host.clone(),
                    project: project.clone(),
                    token: "invalid-token-12345".to_string(),
                    target_project: project.clone(),
                })
                .build()
        }
        ForgeType::Forgejo => {
            let host = std::env::var("FORGEJO_HOST").expect("FORGEJO_HOST required");
            let project = std::env::var("FORGEJO_PROJECT").expect("FORGEJO_PROJECT required");
            let ca_bundle = std::env::var("FORGEJO_CA_BUNDLE").ok();
            let accept_non_compliant = std::env::var("FORGEJO_TLS_ACCEPT_NON_COMPLIANT_CERTS")
                .ok()
                .and_then(|v| v.parse::<bool>().ok())
                .unwrap_or(false);
            config
                .tls_accept_non_compliant_certs(accept_non_compliant)
                .maybe_ca_bundle(ca_bundle)
                .forgejo(crate::config::ForgejoConfig {
                    host: host.clone(),
                    project: project.clone(),
                    token: "invalid-token-12345".to_string(),
                    target_project: project.clone(),
                    wip_prefix: "WIP: ".to_string(),
                })
                .build()
        }
        ForgeType::AzureDevOps => {
            let host = std::env::var("AZURE_HOST").expect("AZURE_HOST required");
            let vssps_host = std::env::var("AZURE_VSSPS_HOST").expect("AZURE_VSSPS_HOST required");
            let project = std::env::var("AZURE_PROJECT").expect("AZURE_PROJECT required");
            let source_repository_name =
                std::env::var("AZURE_REPO_NAME").expect("AZURE_REPO_NAME required");
            let ca_bundle = std::env::var("AZURE_CA_BUNDLE").ok();
            let accept_non_compliant = std::env::var("AZURE_TLS_ACCEPT_NON_COMPLIANT_CERTS")
                .ok()
                .and_then(|v| v.parse::<bool>().ok())
                .unwrap_or(false);
            config
                .tls_accept_non_compliant_certs(accept_non_compliant)
                .maybe_ca_bundle(ca_bundle)
                .azure(crate::config::AzureDevOpsConfig {
                    project: project.clone(),
                    token: "invalid-token-12345".to_string(),
                    host: host.clone(),
                    target_project: project.clone(),
                    source_repository_name: Some(source_repository_name.clone()),
                    target_repository_name: Some(source_repository_name.clone()),
                    source_repository_id: None,
                    target_repository_id: None,
                    vssps_host: vssps_host.clone(),
                })
                .build()
        }
    };

    let forge = ForgeImpl::new(&config)?;

    let result = forge
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

#[rstest]
#[case(ForgeType::GitLab)]
#[case(ForgeType::GitHub)]
#[case(ForgeType::Forgejo)]
#[case(ForgeType::AzureDevOps)]
#[tokio::test]
async fn test_nonexistent_project_errors_clearly(#[case] forge_type: ForgeType) -> Result<()> {
    dotenv::dotenv().ok();

    let config = Config::builder().forge(forge_type);
    let config = match forge_type {
        ForgeType::GitLab => {
            let host = std::env::var("GITLAB_HOST").expect("GITLAB_HOST required");
            let ca_bundle = std::env::var("GITLAB_CA_BUNDLE").ok();
            let accept_non_compliant = std::env::var("GITLAB_TLS_ACCEPT_NON_COMPLIANT_CERTS")
                .ok()
                .and_then(|v| v.parse::<bool>().ok())
                .unwrap_or(false);
            config
                .tls_accept_non_compliant_certs(accept_non_compliant)
                .maybe_ca_bundle(ca_bundle)
                .gitlab(crate::config::GitLabConfig {
                    host: host.clone(),
                    project: "nonexistent/fake-project-12345".to_string(),
                    token: "invalid-token-12345".to_string(),
                    target_project: "nonexistent/fake-project-12345".to_string(),
                    create_merge_request_dependencies: true,
                })
                .build()
        }
        ForgeType::GitHub => {
            let host = std::env::var("GITHUB_HOST")
                .unwrap_or_else(|_| "https://api.github.com".to_string());
            let ca_bundle = std::env::var("GITHUB_CA_BUNDLE").ok();
            let accept_non_compliant = std::env::var("GITHUB_TLS_ACCEPT_NON_COMPLIANT_CERTS")
                .ok()
                .and_then(|v| v.parse::<bool>().ok())
                .unwrap_or(false);

            config
                .tls_accept_non_compliant_certs(accept_non_compliant)
                .maybe_ca_bundle(ca_bundle)
                .github(crate::config::GitHubConfig {
                    host: host.clone(),
                    project: "nonexistent/fake-project-12345".to_string(),
                    token: "invalid-token-12345".to_string(),
                    target_project: "nonexistent/fake-project-12345".to_string(),
                })
                .build()
        }
        ForgeType::Forgejo => {
            let host = std::env::var("FORGEJO_HOST").expect("FORGEJO_HOST required");
            let ca_bundle = std::env::var("FORGEJO_CA_BUNDLE").ok();
            let accept_non_compliant = std::env::var("FORGEJO_TLS_ACCEPT_NON_COMPLIANT_CERTS")
                .ok()
                .and_then(|v| v.parse::<bool>().ok())
                .unwrap_or(false);
            config
                .tls_accept_non_compliant_certs(accept_non_compliant)
                .maybe_ca_bundle(ca_bundle)
                .forgejo(crate::config::ForgejoConfig {
                    host: host.clone(),
                    project: "nonexistent/fake-project-12345".to_string(),
                    token: "invalid-token-12345".to_string(),
                    target_project: "nonexistent/fake-project-12345".to_string(),
                    wip_prefix: "WIP: ".to_string(),
                })
                .build()
        }
        ForgeType::AzureDevOps => {
            let host = std::env::var("AZURE_HOST").expect("AZURE_HOST required");
            let vssps_host = std::env::var("AZURE_VSSPS_HOST").expect("AZURE_VSSPS_HOST required");
            let source_repository_name =
                std::env::var("AZURE_REPO_NAME").expect("AZURE_REPO_NAME required");
            let ca_bundle = std::env::var("AZURE_CA_BUNDLE").ok();
            let accept_non_compliant = std::env::var("AZURE_TLS_ACCEPT_NON_COMPLIANT_CERTS")
                .ok()
                .and_then(|v| v.parse::<bool>().ok())
                .unwrap_or(false);
            config
                .tls_accept_non_compliant_certs(accept_non_compliant)
                .maybe_ca_bundle(ca_bundle)
                .azure(crate::config::AzureDevOpsConfig {
                    project: "nonexistent/fake-project-12345".to_string(),
                    token: "invalid-token-12345".to_string(),
                    host: host.clone(),
                    target_project: "nonexistent/fake-project-12345".to_string(),
                    source_repository_name: Some(source_repository_name.clone()),
                    target_repository_name: Some(source_repository_name.clone()),
                    source_repository_id: None,
                    target_repository_id: None,
                    vssps_host: vssps_host.clone(),
                })
                .build()
        }
    };

    let forge = ForgeImpl::new(&config)?;

    let result = forge
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

#[rstest]
#[case(TestRepo::with_gitlab_remote())]
#[case(TestRepo::with_github_remote())]
#[case(TestRepo::with_forgejo_remote())]
#[case(TestRepo::with_azure_remote())]
#[tokio::test]
async fn test_create_merge_request_dependencies<T: Forge + Clone + Into<ForgeImpl>>(
    #[case] repo: TestRepo<T>,
) -> Result<()> {
    let a = unique_branch("dependency-a");
    let b = unique_branch("dependency-b");
    let c = unique_branch("dependency-c");

    // main -> A -> C
    // main -> B -> C

    repo.new_on("main")
        .create_change_and_bookmark(&a)
        .new_on("main")
        .create_change_and_bookmark(&b)
        .exec(["new", &a, &b])
        .create_change_and_bookmark(&c);

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

    match repo.forge_impl() {
        ForgeImpl::GitLab(forge) => {
            let deps: Vec<_> = forge
                .get_merge_request_dependencies(&mr_c.iid())
                .await?
                .iter()
                .map(|dep| dep.blocking_merge_request.iid)
                .collect();

            assert_eq!(deps.len(), 2);
            assert_contains!(deps, &mr_a.iid().parse::<u64>().unwrap());
            assert_contains!(deps, &mr_b.iid().parse::<u64>().unwrap());
        }
        _ => {}
    };

    Ok(())
}
