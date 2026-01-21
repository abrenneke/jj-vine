use crate::{
    bookmark::{Bookmark, BookmarkGraph, BookmarkRef},
    error::Result,
    tests::TestRepo,
};

#[test]
fn test_deleted_middle_bookmark() -> Result<()> {
    let repo = TestRepo::new();

    // Create stack: a -> b -> c
    repo.commit_with_bookmark("file1.txt", "content1", "Commit A", "bookmark-a")
        .commit_with_bookmark("file2.txt", "content2", "Commit B", "bookmark-b")
        .commit_with_bookmark("file3.txt", "content3", "Commit C", "bookmark-c");

    // Delete middle bookmark
    repo.jj.exec(["bookmark", "delete", "bookmark-b"]).unwrap();

    let changes = repo.jj.log("mine() & bookmarks()")?;
    let bookmarks: Vec<_> = Bookmark::from_changes(&changes).into_iter().collect();

    let graph = BookmarkGraph::from_bookmarks(&repo.jj, bookmarks.iter().cloned(), false)?;

    let bookmark_a = graph.find_bookmark_in_components("bookmark-a").unwrap();

    assert!(graph.find_bookmark_in_components("bookmark-b").is_none());

    let bookmark_c = graph.find_bookmark_in_components("bookmark-c").unwrap();

    assert!(
        bookmark_c
            .parents
            .iter()
            .any(|p| p == &BookmarkRef::Bookmark(bookmark_a.clone()))
    );

    Ok(())
}

#[test]
fn test_base_branch_not_included_in_submission() -> Result<()> {
    let repo = TestRepo::new();
    let upstream = TestRepo::new();
    upstream
        .create_change("init.txt", "initial", "Initial commit")
        .create_bookmark("main");

    repo.jj
        .exec([
            "git",
            "remote",
            "add",
            "origin",
            upstream.path.to_str().unwrap(),
        ])
        .unwrap();

    repo.jj.exec(["git", "fetch"])?;

    repo.jj
        .exec(["bookmark", "track", "main", "--remote", "origin"])?;

    repo.jj.exec(["new", "main"]).unwrap();
    repo.create_change("f1.txt", "feature1", "Feature 1")
        .create_bookmark("feature-1");

    repo.jj.exec(["new"]).unwrap();
    repo.create_change("f2.txt", "feature2", "Feature 2")
        .create_bookmark("feature-2");

    let changes = repo.jj.log("::feature-2")?;
    let bookmarks: Vec<_> = Bookmark::from_changes(&changes).into_iter().collect();

    let graph = BookmarkGraph::from_bookmarks(&repo.jj, bookmarks.iter().cloned(), false)?;
    let stack = graph.component_containing("feature-2").unwrap();

    assert!(stack.contains("feature-1"));
    assert!(stack.contains("feature-2"));
    assert!(!stack.contains("main"));

    Ok(())
}

#[test]
fn test_submit_base_branch_errors() -> Result<()> {
    let repo = TestRepo::new();
    let upstream = TestRepo::new();

    upstream
        .create_change("init.txt", "initial", "Initial commit")
        .create_bookmark("main");

    repo.jj
        .exec([
            "git",
            "remote",
            "add",
            "origin",
            upstream.path.to_str().unwrap(),
        ])
        .unwrap();

    repo.jj.exec(["git", "fetch"])?;

    repo.create_change("feature.txt", "feature", "Feature commit")
        .create_bookmark("feature-1");

    repo.jj
        .exec(["bookmark", "track", "main", "--remote", "origin"])?;

    let changes = repo.jj.log("main")?;
    let bookmarks: Vec<_> = Bookmark::from_changes(&changes).into_iter().collect();
    let graph = BookmarkGraph::from_bookmarks(&repo.jj, bookmarks.iter().cloned(), true)?;

    assert!(graph.components().is_empty(), "Should have no stacks");

    Ok(())
}

#[test]
fn test_graph_skips_default_branch_history() -> Result<()> {
    let repo = TestRepo::new();
    let upstream = TestRepo::new();
    upstream
        .create_change("init.txt", "initial", "Initial commit")
        .create_bookmark("main");

    repo.jj
        .exec([
            "git",
            "remote",
            "add",
            "origin",
            upstream.path.to_str().unwrap(),
        ])
        .unwrap();

    repo.jj.exec(["git", "fetch"])?;

    repo.jj.exec(["bookmark", "track", "main"])?;

    repo.jj.exec(["new", "main"])?;

    for i in 1..=50 {
        repo.create_change(
            &format!("file{}.txt", i),
            &format!("content {}", i),
            &format!("Commit {}", i),
        );
        repo.jj.exec(["commit", "-m", &format!("Commit {}", i)])?;
    }

    repo.jj.exec(["bookmark", "set", "main", "--to", "@-"])?;
    repo.jj.exec(["git", "push"])?;

    repo.jj.exec(["new", "main"])?;
    repo.create_change("feature.txt", "feature", "Feature commit")
        .create_bookmark("feature-1");

    let changes = repo.jj.log("mine() & bookmarks()")?;
    let bookmarks: Vec<_> = Bookmark::from_changes(&changes).into_iter().collect();

    let graph = BookmarkGraph::from_bookmarks(&repo.jj, bookmarks.iter().cloned(), false)?;

    assert!(graph.component_containing("main").is_none());
    assert!(graph.component_containing("feature-1").is_some());

    Ok(())
}

#[cfg(not(feature = "no-e2e-tests"))]
mod e2e {
    use crate::{
        error::Result,
        tests::{TestRepo, unique_branch},
    };

    #[tokio::test]
    async fn test_multiple_independent_stacks_dont_incorrectly_retarget() -> Result<()> {
        let repo = TestRepo::with_gitlab_remote();

        // Create two independent stacks:
        // Stack 1: main -> stack1-a -> stack1-b
        // Stack 2: main -> stack2-a

        // Stack 1, bookmark A
        repo.jj.exec(["new", "main"])?;
        let branch_1a = unique_branch("stack1-a");
        repo.create_change("file1.txt", "stack1-a content", "Stack 1 A")
            .create_and_push_bookmark(&branch_1a);

        // Stack 1, bookmark B
        repo.jj.exec(["new"])?;
        let branch_1b = unique_branch("stack1-b");
        repo.create_change("file2.txt", "stack1-b content", "Stack 1 B")
            .create_and_push_bookmark(&branch_1b);

        // Stack 2, bookmark A (independent from stack 1)
        repo.jj.exec(["new", "main"])?;
        let branch_2a = unique_branch("stack2-a");
        repo.create_change("file3.txt", "stack2-a content", "Stack 2 A")
            .create_and_push_bookmark(&branch_2a);

        // Dry run submission to see what the tool wants to do
        let output = repo
            .submit(crate::commands::submit::SubmitCommandConfig {
                tracked: true,
                dry_run: true,
                ..Default::default()
            })
            .await;

        // Verify that stack2-a targets main, not stack1-b
        assert!(
            output.contains(&format!("Would create {} -> main", branch_1a)),
            "{} should target main, output:\n{}",
            branch_1a,
            output
        );
        assert!(
            output.contains(&format!("Would create {} -> {}", branch_1b, branch_1a)),
            "{} should target {}, output:\n{}",
            branch_1b,
            branch_1a,
            output
        );
        assert!(
            output.contains(&format!("Would create {} -> main", branch_2a)),
            "{} should target main (not {}!), output:\n{}",
            branch_2a,
            branch_1b,
            output
        );

        Ok(())
    }
}
