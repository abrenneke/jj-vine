use std::{borrow::Cow, collections::HashSet};

use clap::Args;
use cli_table::{
    Cell,
    Table,
    format::{Border, Separator},
};
use itertools::Itertools;
use owo_colors::OwoColorize;
use snafu::{ensure_whatever, whatever};
use tracing::info;
use unicode_segmentation::UnicodeSegmentation;

use crate::{
    bookmark::{BookmarkGraph, BookmarkOrPending},
    cli::CliConfig,
    commands::{GetBookmarksOptions, StrVisualWidth},
    config::Config,
    error::{AggregateSnafu, Result},
    forge::ForgeImpl,
    jj::Jujutsu,
    submit::{
        PlanContext,
        RootExecuteContext,
        execute::{self, MRUpdate, MRUpdateType},
        find_changes_to_submit,
        plan,
    },
};

#[derive(Args, Default)]
pub struct SubmitCommandConfig {
    /// Options for the revset
    #[command(flatten)]
    pub revset_options: SubmitCommandRevsetOptions,

    /// The remote to push to. Defaults to the `git.push` or `git.fetch`
    /// settings.
    #[arg(long)]
    pub remote: Option<String>,

    /// Don't actually modify any merge requests or push bookmarks, only print
    /// what would be done.
    #[arg(long)]
    pub dry_run: bool,

    /// Show the submission plan and do not execute it.
    #[arg(long, conflicts_with = "dry_run")]
    pub show_plan: bool,

    /// Create bookmarks for changes that don't have one (via `jj git push -c`).
    /// A bookmark will be created for each revision in the revset for this
    /// parameter, intersected with the main revset being submitted
    /// ([create] & [revset]). If `--create` is passed without a value, the
    /// value will default to `all()`, which will create one bookmark for each
    /// revision in the revset being submitted, if it does not already have one.
    ///
    /// Revisions will be skipped if they already have a bookmark, tracked or
    /// not.
    ///
    /// For example, if you have revision A off of `trunk()`, and B and C off of
    /// A, then:
    ///
    /// `jj-vine submit 'trunk()..' -c` -> creates bookmarks for A, B, and
    /// C
    ///
    /// `jj-vine submit 'trunk()..' -c C -> only creates a bookmark for C, so
    /// the pull/merge request for C contains both A and C. A is not pushed
    /// nor a pull or merge request created.
    ///
    /// You may also not specify a revset, and just use -c, in which case the
    /// value of -c will be used as the submitting revset. For example,
    /// `jj-vine submit -c @` is equivalent to `jj git push -c @ && jj-vine
    /// submit @` or `jj-vine submit -r @ -c @`.
    #[arg(short = 'c', long = "create", num_args=0..=1, require_equals=true, default_missing_value="all()")]
    pub create: Option<String>,
}

impl SubmitCommandConfig {
    pub fn help_long() -> String {
        format!(
            r#"
Submit one or more bookmarks to the code forge.
This command will create merge requests that don't exist, update
existing merge requests to the correct target branch, and sync all
merge request descriptions.

{}

Submit a single bookmark:
{}

Submit all tracked bookmarks:
{}

Preview submitting a revset without making changes:
{}
"#,
            "Examples:".yellow().bold(),
            "jj vine submit <bookmark>".green().bold(),
            "jj vine submit --tracked".green().bold(),
            "jj vine submit -r <revset> --dry-run".green().bold(),
        )
        .trim()
        .to_string()
    }
}

#[derive(Args, Default)]
#[group(required = false, multiple = false)]
pub struct SubmitCommandRevsetOptions {
    /// The revset to submit (may use -r or not).
    #[arg(id = "revset")]
    pub revset_positional: Option<String>,

    /// The revset to submit (may use -r or not).
    #[arg(id = "revset_arg", short = 'r', long)]
    pub revset: Option<String>,

    /// Submit all tracked bookmarks.
    ///
    /// While this is roughly equivalent to
    /// `(mine() & tracked_remote_bookmarks()) ~ trunk()`, it includes the
    /// additional stipulation that all submitted bookmarks must be already
    /// pushed to the remote. Bookmarks which have non-tracked parents or
    /// children will be skipped over.
    #[arg(short = 't', long)]
    pub tracked: bool,
}

impl SubmitCommandConfig {
    fn to_get_bookmarks_options(&self) -> Result<GetBookmarksOptions> {
        match (
            self.revset_options.revset_positional.as_deref(),
            self.revset_options.revset.as_deref(),
            self.revset_options.tracked,
            self.create.as_deref(),
        ) {
            (Some(revset), None, false, _) => Ok(GetBookmarksOptions::Revset(revset.to_string())),
            (None, Some(revset), false, _) => Ok(GetBookmarksOptions::Revset(revset.to_string())),
            (None, None, true, _) => Ok(GetBookmarksOptions::Tracked),

            // Fall back to the same as -c if none of the other options are set
            (None, None, false, Some(create)) => {
                Ok(GetBookmarksOptions::Revset(create.to_string()))
            }
            _ => whatever!(
                "You must specify a revset to submit with a positional argument, with the -r option, or with the --tracked option. You can also use the -c option to create bookmarks for changes that don't have one."
            ),
        }
    }
}

pub async fn submit(config: &SubmitCommandConfig, cli_config: &CliConfig<'_>) -> Result<()> {
    let jj = Jujutsu::new(&cli_config.repository)?;
    let output = cli_config.output;

    let repo_config = Config::load(&cli_config.repository)?;

    if repo_config.fetch {
        if config.dry_run {
            output.log_message(&format!(
                "Would fetch remote{} before planning (note that the plan may change based on newly fetched data!)",
                config.remote.as_deref().map(|r| format!(" {r}")).unwrap_or_default())
            );
        } else {
            let mut fetch_args = vec!["git", "fetch"];
            if let Some(remote) = config.remote.as_deref() {
                fetch_args.extend_from_slice(&["--remote", remote]);
            }

            jj.exec(fetch_args)?;
        }
    }

    let revset = config.to_get_bookmarks_options()?.to_revset();

    let mut pending_bookmarks = HashSet::new();
    if let Some(create) = config.create.as_deref() {
        output.log_current("Creating and pushing bookmarks");

        let changes_to_create_bookmarks_for = jj.log(format!("({create}) & ({revset})"))?;

        if changes_to_create_bookmarks_for.is_empty() {
            whatever!(
                "Your change parameter resolved to a revset ({}) & ({}), which is empty. This is probably not what you intended, as no bookmarks would be created. Not continuing with submit.",
                create,
                revset
            );
        }

        pending_bookmarks.extend(
            changes_to_create_bookmarks_for
                .iter()
                .filter(|c| c.bookmarks.is_empty())
                .map(|c| c.change_id.clone()),
        );
    }

    let changes = jj.log_with_pending_bookmarks(&revset, &pending_bookmarks)?;

    let bookmarks: Vec<_> = BookmarkOrPending::from_changes(&changes)
        .into_iter()
        .collect();

    ensure_whatever!(!bookmarks.is_empty(), "No bookmarks in revset {}", revset);

    let forge = ForgeImpl::new(&repo_config)?;

    output.log_message(&format!(
        "Submitting bookmarks{}: {}",
        if config.dry_run {
            " (dry run)"
        } else if config.show_plan {
            " (plan only)"
        } else {
            ""
        },
        bookmarks.iter().map(|b| b.magenta().to_string()).join(", ")
    ));

    let changes = find_changes_to_submit(
        &jj,
        bookmarks.iter().map(|b| b.change_id()),
        &pending_bookmarks,
    )?;

    let bookmark_graph = BookmarkGraph::from_changes(&jj, &changes, config.revset_options.tracked)?;

    let submission_plan = plan::plan(PlanContext {
        jj: &jj,
        forge: &forge,
        config: &repo_config,
        output,
        bookmark_graph: &bookmark_graph,
        dry_run: config.dry_run,
    })
    .await?;

    if config.show_plan {
        info!("Submission plan:");
        info!("{}", submission_plan);
        return Ok(());
    }

    let result = execute::execute(RootExecuteContext::new(
        &jj,
        &forge,
        &repo_config,
        output,
        config.dry_run,
        submission_plan,
        changes.clone(),
        config.revset_options.tracked,
    ))
    .await?;

    output.finish();

    info!("\n═══════════════════════════════════════");
    info!("{}", "Summary".bold());
    info!("═══════════════════════════════════════");

    if !result.bookmarks_pushed.is_empty() {
        let formatted_bookmarks: Vec<String> = result
            .bookmarks_pushed
            .iter()
            .map(|b| b.magenta().to_string())
            .collect();
        info!("Pushed: {}", formatted_bookmarks.join(", "));
    } else {
        info!("No bookmarks pushed");
    }

    if !result.merge_requests.is_empty() {
        info!("\n{}\n", "Merge Requests:".bold());

        let mut table = vec![];

        let mut updates: Vec<_> = result
            .merge_requests
            .iter()
            .sorted_by_key(|mr| mr.mr.iid())
            .collect();

        // If an MR was just created, don't also report that it was updated
        for (index, update) in updates.clone().into_iter().enumerate().rev() {
            if let MRUpdateType::Updated { .. } = update.update_type
                && updates
                    .iter()
                    .find(|u| {
                        u.mr.iid() == update.mr.iid()
                            && matches!(u.update_type, MRUpdateType::Created)
                    })
                    .is_some()
            {
                updates.remove(index);
            }
        }

        updates.dedup_by_key(|u| u.mr.iid());

        for MRUpdate {
            mr,
            bookmark,
            update_type,
        } in updates
        {
            match update_type {
                MRUpdateType::Created => {
                    table.push(vec![
                        bookmark.magenta().cell(),
                        mr.title().wrap(60).cell(),
                        mr.edit_url().dimmed().cell(),
                        "[created]".green().cell(),
                    ]);
                }
                MRUpdateType::Updated { .. } => {
                    table.push(vec![
                        bookmark.magenta().cell(),
                        mr.title().wrap(60).cell(),
                        mr.url().dimmed().cell(),
                        "[updated]".green().cell(),
                    ]);
                }
                MRUpdateType::Unchanged => {
                    table.push(vec![
                        bookmark.magenta().cell(),
                        mr.title().wrap(60).cell(),
                        mr.url().dimmed().cell(),
                        " ".cell(),
                    ]);
                }
            }
        }

        info!(
            "{}",
            table
                .table()
                .border(Border::builder().build())
                .separator(Separator::builder().build())
                .display()
                .expect("Failed to display table")
        );
    }

    if !result.errors.is_empty() {
        info!("");
        info!("✗ {} error(s) occurred:", result.errors.len());
        for error in &result.errors {
            info!("  • {}", error);
        }
    }

    if !result.errors.is_empty() {
        return Err(AggregateSnafu {
            errors: result
                .errors
                .into_iter()
                .map::<Box<dyn std::error::Error + 'static>, _>(|e| Box::new(e))
                .collect::<Vec<_>>(),
        }
        .build());
    }

    Ok(())
}

trait WrapText {
    /// Wrap text to the given width by adding newlines at word boundaries
    fn wrap(&self, max_width: usize) -> Cow<'_, str>;
}

impl<T> WrapText for T
where
    T: AsRef<str>,
{
    /// Wrap text to the given width by adding newlines at word boundaries
    fn wrap(&self, max_width: usize) -> Cow<'_, str> {
        if self.visual_width() <= max_width {
            return Cow::Borrowed(self.as_ref());
        }

        let mut lines = Vec::new();
        let mut current = String::new();

        for word in self.as_ref().split_word_bounds() {
            if current.visual_width() + word.visual_width() > max_width {
                lines.push(current);
                current = word.trim_start().to_string();
            } else {
                current.push_str(word);
            }
        }

        if !current.is_empty() {
            lines.push(current);
        }

        Cow::Owned(lines.join("\n"))
    }
}
