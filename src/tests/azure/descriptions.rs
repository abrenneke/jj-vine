use assertables::assert_contains;

use crate::{
    description::{END_MARKER, START_MARKER},
    error::Result,
    forge::Forge,
    tests::TestRepo,
};

#[tokio::test]
async fn test_pr_description_includes_stack_info() -> Result<()> {
    let repo = TestRepo::with_azure_remote();

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

    assert_contains!(pr.description, START_MARKER);
    assert_contains!(pr.description, END_MARKER);

    Ok(())
}

#[tokio::test]
async fn test_pr_description_links_to_dependent_prs() -> Result<()> {
    let repo = TestRepo::with_azure_remote();

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

    let pr_a = repo
        .forge()
        .find_merge_request_by_source_branch(&branch_a)
        .await?
        .expect("PR A should exist");

    let pr_b = repo
        .forge()
        .find_merge_request_by_source_branch(&branch_b)
        .await?
        .expect("PR B should exist");

    assert_contains!(pr_a.description, &format!("!{}", pr_b.pull_request_id));
    assert_contains!(pr_b.description, &format!("!{}", pr_a.pull_request_id));

    Ok(())
}

#[tokio::test]
async fn test_user_content_preserved_on_resubmit() -> Result<()> {
    let repo = TestRepo::with_azure_remote();

    let branch_a = repo.bookmark_name("preserve-a");
    let branch_b = repo.bookmark_name("preserve-b");

    repo.jj.exec(["new", "main"])?;
    repo.create_change("a.txt", "a", "Commit A")
        .create_and_push_bookmark(&branch_a);

    repo.run(["submit", &branch_a]).await;

    let pr_a = repo
        .forge()
        .find_merge_request_by_source_branch(&branch_a)
        .await?
        .expect("PR A should exist");

    let user_content = "My important notes about this PR";
    let new_desc = format!("{}\n\n{}", pr_a.description, user_content);
    repo.forge()
        .update_merge_request_info(pr_a.pull_request_id, &new_desc, &pr_a.title)
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
            .description,
        user_content
    );

    Ok(())
}

#[tokio::test]
async fn test_add_markers_to_description_without_markers() -> Result<()> {
    let repo = TestRepo::with_azure_remote();

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
        .update_merge_request_info(pr.pull_request_id, user_description, &pr.title)
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

    assert_contains!(pr.description, START_MARKER);
    assert_contains!(pr.description, END_MARKER);
    assert_contains!(pr.description, user_description);

    Ok(())
}

#[tokio::test]
async fn test_skip_update_when_description_unchanged() -> Result<()> {
    let repo = TestRepo::with_azure_remote();

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
        .description
        .to_string();

    repo.run(["submit", &branch_b]).await;

    assert_eq!(
        initial_desc,
        repo.forge()
            .find_merge_request_by_source_branch(&branch_a)
            .await?
            .expect("PR A should exist")
            .description
    );

    Ok(())
}
