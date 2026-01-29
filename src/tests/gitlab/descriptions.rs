use assertables::assert_contains;

use crate::{
    description::{END_MARKER, START_MARKER},
    error::Result,
    forge::Forge,
    tests::{TestRepo, unique_branch},
};

#[tokio::test]
async fn test_mr_description_includes_stack_info() -> Result<()> {
    let repo = TestRepo::with_gitlab_remote();

    let branch_a = unique_branch("desc-a");
    let branch_b = unique_branch("desc-b");

    // main -> A -> B
    repo.jj
        .exec(["new", "main"])
        .expect("Failed to create main branch");
    repo.create_change("a.txt", "a", "Commit A")
        .create_and_push_bookmark(&branch_a);

    repo.jj.exec(["new"]).expect("Failed to create new branch");
    repo.create_change("b.txt", "b", "Commit B")
        .create_and_push_bookmark(&branch_b);

    repo.run(["submit", &branch_b]).await;

    let mr = repo
        .forge()
        .find_merge_request_by_source_branch(&branch_a)
        .await?
        .expect("MR A should exist");

    assert_contains!(mr.description.as_ref().unwrap(), START_MARKER);
    assert_contains!(mr.description.as_ref().unwrap(), END_MARKER);

    Ok(())
}

#[tokio::test]
async fn test_mr_description_links_to_dependent_mrs() -> Result<()> {
    let repo = TestRepo::with_gitlab_remote();

    let branch_a = unique_branch("link-a");
    let branch_b = unique_branch("link-b");

    // main -> A -> B
    repo.jj
        .exec(["new", "main"])
        .expect("Failed to create main branch");
    repo.create_change("a.txt", "a", "Commit A")
        .create_and_push_bookmark(&branch_a);

    repo.jj.exec(["new"]).expect("Failed to create new branch");
    repo.create_change("b.txt", "b", "Commit B")
        .create_and_push_bookmark(&branch_b);

    repo.run(["submit", &branch_b]).await;

    let mr_a = repo
        .forge()
        .find_merge_request_by_source_branch(&branch_a)
        .await?
        .expect("MR A should exist");

    let mr_b = repo
        .forge()
        .find_merge_request_by_source_branch(&branch_b)
        .await?
        .expect("MR B should exist");

    assert_contains!(
        mr_a.description.as_ref().unwrap(),
        &format!("!{}", mr_b.iid)
    );

    Ok(())
}

#[tokio::test]
async fn test_user_content_preserved_on_resubmit() -> Result<()> {
    let repo = TestRepo::with_gitlab_remote();

    let branch_a = unique_branch("preserve-a");
    let branch_b = unique_branch("preserve-b");

    // main -> A
    repo.jj
        .exec(["new", "main"])
        .expect("Failed to create main branch");
    repo.create_change("a.txt", "a", "Commit A")
        .create_and_push_bookmark(&branch_a);

    repo.run(["submit", &branch_a]).await;

    let mr = repo
        .forge()
        .find_merge_request_by_source_branch(&branch_a)
        .await?
        .expect("MR A should exist");

    let user_content = "My important notes about this MR";
    let new_desc = format!("{}\n\n{}", mr.description.unwrap(), user_content);
    repo.forge()
        .update_merge_request_description(mr.iid, &new_desc)
        .await?;

    repo.jj.exec(["new"])?;
    repo.create_change("b.txt", "b", "Commit B")
        .create_and_push_bookmark(&branch_b);

    repo.run(["submit", &branch_b]).await;

    let mr_updated = repo
        .forge()
        .find_merge_request_by_source_branch(&branch_a)
        .await?
        .expect("MR A should exist");

    assert_contains!(mr_updated.description.unwrap(), user_content);

    Ok(())
}

#[tokio::test]
async fn test_add_markers_to_description_without_markers() -> Result<()> {
    let repo = TestRepo::with_gitlab_remote();

    let branch_a = unique_branch("markers-a");
    let branch_b = unique_branch("markers-b");

    // main -> A
    repo.jj
        .exec(["new", "main"])
        .expect("Failed to create main branch");
    repo.create_change("a.txt", "a", "Commit A")
        .create_and_push_bookmark(&branch_a);

    repo.run(["submit", &branch_a]).await;

    let mr = repo
        .forge()
        .find_merge_request_by_source_branch(&branch_a)
        .await?
        .expect("MR A should exist");

    let user_description = "Custom description without markers";
    repo.forge()
        .update_merge_request_description(mr.iid, user_description)
        .await?;

    repo.jj.exec(["new"])?;
    repo.create_change("b.txt", "b", "Commit B")
        .create_and_push_bookmark(&branch_b);
    repo.run(["submit", &branch_b]).await;

    let mr = repo
        .forge()
        .find_merge_request_by_source_branch(&branch_a)
        .await?
        .expect("MR A should exist");

    assert_contains!(mr.description.as_ref().unwrap(), START_MARKER);
    assert_contains!(mr.description.as_ref().unwrap(), END_MARKER);
    assert_contains!(mr.description.as_ref().unwrap(), user_description);

    Ok(())
}

#[tokio::test]
async fn test_skip_update_when_description_unchanged() -> Result<()> {
    let repo = TestRepo::with_gitlab_remote();

    let branch_a = unique_branch("unchanged-a");
    let branch_b = unique_branch("unchanged-b");

    // main -> A -> B
    repo.jj
        .exec(["new", "main"])
        .expect("Failed to create main branch");
    repo.create_change("a.txt", "a", "Commit A")
        .create_and_push_bookmark(&branch_a);

    repo.jj.exec(["new"]).expect("Failed to create new branch");
    repo.create_change("b.txt", "b", "Commit B")
        .create_and_push_bookmark(&branch_b);

    repo.run(["submit", &branch_b]).await;

    let mr = repo
        .forge()
        .find_merge_request_by_source_branch(&branch_a)
        .await?
        .expect("MR A should exist");

    repo.run(["submit", &branch_b]).await;

    let mr_after = repo
        .forge()
        .find_merge_request_by_source_branch(&branch_a)
        .await?
        .expect("MR A should exist");

    assert_eq!(mr.description, mr_after.description);

    Ok(())
}
