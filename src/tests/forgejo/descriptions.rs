use assertables::assert_contains;

use crate::{
    description::{END_MARKER, START_MARKER},
    error::Result,
    forge::{Forge, ForgeMergeRequest, ForgeUpdateMergeRequestInfoOptions},
    tests::TestRepo,
};

#[tokio::test]
async fn test_pr_description_includes_stack_info() -> Result<()> {
    let repo = TestRepo::with_forgejo_remote();

    let branch_a = repo.bookmark_name("desc-a");
    let branch_b = repo.bookmark_name("desc-b");

    // main -> A -> B
    repo.jj.exec(["new", "main"])?;
    repo.create_change("a.txt", "a", "Commit A")
        .create_and_push_bookmark(&branch_a);

    repo.jj.exec(["new"])?;
    repo.create_change("b.txt", "b", "Commit B")
        .create_and_push_bookmark(&branch_b);

    repo.run(["submit", &branch_b]).await;

    let pr = repo
        .forge()
        .find_merge_request_by_source_branch(&branch_a)
        .await?
        .expect("PR A should exist");

    assert_contains!(pr.pull_request.body.as_ref().unwrap(), START_MARKER);
    assert_contains!(pr.pull_request.body.as_ref().unwrap(), END_MARKER);

    Ok(())
}

#[tokio::test]
async fn test_pr_description_links_to_dependent_prs() -> Result<()> {
    let repo = TestRepo::with_forgejo_remote();

    let branch_a = repo.bookmark_name("link-a");
    let branch_b = repo.bookmark_name("link-b");

    // main -> A -> B
    repo.jj.exec(["new", "main"])?;
    repo.create_change("a.txt", "a", "Commit A")
        .create_and_push_bookmark(&branch_a);

    repo.jj.exec(["new"])?;
    repo.create_change("b.txt", "b", "Commit B")
        .create_and_push_bookmark(&branch_b);

    repo.run(["submit", &branch_b]).await;

    let a = repo
        .forge()
        .find_merge_request_by_source_branch(&branch_a)
        .await?
        .expect("PR A should exist");

    let b = repo
        .forge()
        .find_merge_request_by_source_branch(&branch_b)
        .await?
        .expect("PR B should exist");

    assert_contains!(
        a.pull_request.body.as_ref().unwrap(),
        &format!("#{}", b.pull_request.number)
    );
    assert_contains!(
        b.pull_request.body.as_ref().unwrap(),
        &format!("#{}", a.pull_request.number)
    );

    Ok(())
}

#[tokio::test]
async fn test_user_content_preserved_on_resubmit() -> Result<()> {
    let repo = TestRepo::with_forgejo_remote();

    let branch_a = repo.bookmark_name("preserve-a");
    let branch_b = repo.bookmark_name("preserve-b");

    // main -> A
    repo.jj.exec(["new", "main"])?;
    repo.create_change("a.txt", "a", "Commit A")
        .create_and_push_bookmark(&branch_a);

    repo.run(["submit", &branch_a]).await;

    let pr = repo
        .forge()
        .find_merge_request_by_source_branch(&branch_a)
        .await?
        .expect("PR A should exist");

    let user_content = "My important notes about this PR";
    let new_desc = format!(
        "{}\n\n{}",
        pr.pull_request.body.as_ref().unwrap(),
        user_content
    );
    repo.forge()
        .update_merge_request_info(
            pr.pull_request.number,
            ForgeUpdateMergeRequestInfoOptions::builder()
                .description(new_desc)
                .current_is_draft(pr.is_draft())
                .current_title(pr.title().to_string())
                .build(),
        )
        .await?;

    repo.jj.exec(["new"])?;
    repo.create_change("b.txt", "b", "Commit B")
        .create_and_push_bookmark(&branch_b);

    repo.run(["submit", &branch_b]).await;

    assert_contains!(
        repo.forge()
            .find_merge_request_by_source_branch(&branch_a)
            .await?
            .expect("PR A should exist")
            .pull_request
            .body
            .as_ref()
            .unwrap(),
        user_content
    );

    Ok(())
}

#[tokio::test]
async fn test_add_markers_to_description_without_markers() -> Result<()> {
    let repo = TestRepo::with_forgejo_remote();

    let branch_a = repo.bookmark_name("markers-a");
    let branch_b = repo.bookmark_name("markers-b");

    // main -> A
    repo.jj.exec(["new", "main"])?;
    repo.create_change("a.txt", "a", "Commit A")
        .create_and_push_bookmark(&branch_a);

    repo.run(["submit", &branch_a]).await;

    let pr = repo
        .forge()
        .find_merge_request_by_source_branch(&branch_a)
        .await?
        .expect("PR A should exist");

    let user_description = "Custom description without markers";
    repo.forge()
        .update_merge_request_info(
            pr.pull_request.number,
            ForgeUpdateMergeRequestInfoOptions::builder()
                .description(user_description.to_string())
                .current_is_draft(pr.is_draft())
                .current_title(pr.title().to_string())
                .build(),
        )
        .await?;

    repo.jj.exec(["new"])?;
    repo.create_change("b.txt", "b", "Commit B")
        .create_and_push_bookmark(&branch_b);

    repo.run(["submit", &branch_b]).await;

    let pr = repo
        .forge()
        .find_merge_request_by_source_branch(&branch_a)
        .await?
        .expect("PR A should exist");

    assert_contains!(pr.pull_request.body.as_ref().unwrap(), START_MARKER);
    assert_contains!(pr.pull_request.body.as_ref().unwrap(), END_MARKER);
    assert_contains!(pr.pull_request.body.as_ref().unwrap(), user_description);

    Ok(())
}

#[tokio::test]
async fn test_skip_update_when_description_unchanged() -> Result<()> {
    let repo = TestRepo::with_forgejo_remote();

    let branch_a = repo.bookmark_name("unchanged-a");
    let branch_b = repo.bookmark_name("unchanged-b");

    // main -> A -> B
    repo.jj.exec(["new", "main"])?;
    repo.create_change("a.txt", "a", "Commit A")
        .create_and_push_bookmark(&branch_a);

    repo.jj.exec(["new"])?;
    repo.create_change("b.txt", "b", "Commit B")
        .create_and_push_bookmark(&branch_b);

    repo.run(["submit", &branch_b]).await;

    let initial_desc = repo
        .forge()
        .find_merge_request_by_source_branch(&branch_a)
        .await?
        .expect("PR A should exist")
        .pull_request
        .body
        .as_ref()
        .unwrap()
        .to_string();

    repo.run(["submit", &branch_b]).await;

    let pr_a_after = repo
        .forge()
        .find_merge_request_by_source_branch(&branch_a)
        .await?
        .expect("PR A should exist");

    assert_eq!(
        initial_desc,
        *pr_a_after.pull_request.body.as_ref().unwrap()
    );

    Ok(())
}

#[tokio::test]
async fn test_sync_description() -> Result<()> {
    let repo = TestRepo::with_forgejo_remote();

    repo.set_config("jj-vine.description.sync", "true");

    let branch_a = repo.bookmark_name("feature-a");
    let branch_b = repo.bookmark_name("feature-b");

    // main -> A -> B
    repo.submit_stack([&branch_a, &branch_b]).await;

    let pr_a = repo.get_mr_with_base(&branch_a, "main").await;
    let pr_b = repo.get_mr_with_base(&branch_b, &branch_a).await;

    assert!(
        pr_a.pull_request
            .body
            .as_ref()
            .unwrap()
            .starts_with(&format!(
                r#"Description for {branch_a} bookmark

<!-- start jj-vine stack -->"#
            ))
    );

    assert!(
        pr_b.pull_request
            .body
            .as_ref()
            .unwrap()
            .starts_with(&format!(
                r#"Description for {branch_b} bookmark

<!-- start jj-vine stack -->"#
            ))
    );

    repo.jj.exec([
        "describe",
        "-r",
        &branch_a,
        "-m",
        "This is the new branch A description\n\nThis is the new branch A body",
    ])?;
    repo.jj.exec([
        "describe",
        "-r",
        &branch_b,
        "-m",
        "This is the new branch B description\n\nThis is the new branch B body",
    ])?;

    repo.run(["submit", &branch_b]).await;

    let pr_a = repo.get_mr_with_base(&branch_a, "main").await;
    let pr_b = repo.get_mr_with_base(&branch_b, &branch_a).await;

    assert!(pr_a.pull_request.body.as_ref().unwrap().starts_with(
        r#"This is the new branch A body

<!-- start jj-vine stack -->"#
    ));

    assert!(pr_b.pull_request.body.as_ref().unwrap().starts_with(
        r#"This is the new branch B body

<!-- start jj-vine stack -->"#
    ));

    Ok(())
}

#[tokio::test]
async fn test_sync_description_disabled() -> Result<()> {
    let repo = TestRepo::with_forgejo_remote();

    repo.set_config("jj-vine.description.sync", "false");

    let branch_a = repo.bookmark_name("feature-a");
    let branch_b = repo.bookmark_name("feature-b");

    // main -> A -> B
    repo.submit_stack([&branch_a, &branch_b]).await;

    let pr_a = repo.get_mr_with_base(&branch_a, "main").await;
    let pr_b = repo.get_mr_with_base(&branch_b, &branch_a).await;

    assert!(
        pr_a.pull_request
            .body
            .as_ref()
            .unwrap()
            .starts_with(&format!(
                r#"Description for {branch_a} bookmark

<!-- start jj-vine stack -->"#
            ))
    );

    assert!(
        pr_b.pull_request
            .body
            .as_ref()
            .unwrap()
            .starts_with(&format!(
                r#"Description for {branch_b} bookmark

<!-- start jj-vine stack -->"#
            ))
    );

    repo.jj.exec([
        "describe",
        "-r",
        &branch_a,
        "-m",
        "This is the new branch A description\n\nThis is the new branch A body",
    ])?;
    repo.jj.exec([
        "describe",
        "-r",
        &branch_b,
        "-m",
        "This is the new branch B description\n\nThis is the new branch B body",
    ])?;

    repo.run(["submit", &branch_b]).await;

    let pr_a = repo.get_mr_with_base(&branch_a, "main").await;
    let pr_b = repo.get_mr_with_base(&branch_b, &branch_a).await;

    assert!(
        pr_a.pull_request
            .body
            .as_ref()
            .unwrap()
            .starts_with(&format!(
                r#"Description for {branch_a} bookmark

<!-- start jj-vine stack -->"#
            ))
    );

    assert!(
        pr_b.pull_request
            .body
            .as_ref()
            .unwrap()
            .starts_with(&format!(
                r#"Description for {branch_b} bookmark

<!-- start jj-vine stack -->"#
            ))
    );

    Ok(())
}
