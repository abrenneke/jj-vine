use assertables::assert_contains;

use crate::{
    description::{END_MARKER, START_MARKER},
    error::Result,
    forge::{Forge, ForgeUpdateMergeRequestInfoOptions},
    tests::TestRepo,
};

#[tokio::test]
async fn test_pr_description_includes_stack_info() -> Result<()> {
    let repo = TestRepo::with_github_remote();

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

    assert_contains!(pr.body.as_ref().unwrap(), START_MARKER);
    assert_contains!(pr.body.as_ref().unwrap(), END_MARKER);

    Ok(())
}

#[tokio::test]
async fn test_pr_description_links_to_dependent_prs() -> Result<()> {
    let repo = TestRepo::with_github_remote();

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

    assert_contains!(a.body.unwrap(), &format!("#{}", b.number));
    assert_contains!(b.body.unwrap(), &format!("#{}", a.number));

    Ok(())
}

#[tokio::test]
async fn test_user_content_preserved_on_resubmit() -> Result<()> {
    let repo = TestRepo::with_github_remote();

    let branch_a = repo.bookmark_name("preserve-a");
    let branch_b = repo.bookmark_name("preserve-b");

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
    repo.forge()
        .update_merge_request_info(
            pr.number,
            ForgeUpdateMergeRequestInfoOptions::builder()
                .description(user_content.to_string())
                .current_is_draft(pr.draft)
                .current_title(pr.title.clone())
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
            .body
            .unwrap(),
        user_content
    );

    Ok(())
}

#[tokio::test]
async fn test_add_markers_to_description_without_markers() -> Result<()> {
    let repo = TestRepo::with_github_remote();

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
            pr.number,
            ForgeUpdateMergeRequestInfoOptions::builder()
                .description(user_description.to_string())
                .current_is_draft(pr.draft)
                .current_title(pr.title.clone())
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

    assert_contains!(pr.body.as_ref().unwrap(), START_MARKER);
    assert_contains!(pr.body.as_ref().unwrap(), END_MARKER);
    assert_contains!(pr.body.as_ref().unwrap(), user_description);

    Ok(())
}

#[tokio::test]
async fn test_skip_update_when_description_unchanged() -> Result<()> {
    let repo = TestRepo::with_github_remote();

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
        .body
        .unwrap()
        .clone();

    repo.run(["submit", &branch_b]).await;

    assert_eq!(
        initial_desc,
        repo.forge()
            .find_merge_request_by_source_branch(&branch_a)
            .await?
            .expect("PR A should exist")
            .body
            .unwrap()
    );

    Ok(())
}
