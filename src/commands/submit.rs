use std::borrow::Cow;

use clap::Args;
use cli_table::{
    Cell,
    Table,
    format::{Border, Separator},
};
use itertools::Itertools;
use owo_colors::OwoColorize;
use snafu::ensure_whatever;
use tracing::info;
use unicode_segmentation::UnicodeSegmentation;

use crate::{
    bookmark::{Bookmark, BookmarkGraph},
    cli::CliConfig,
    commands::{GetBookmarksOptions, StrVisualWidth},
    config::Config,
    error::{AggregateSnafu, Result},
    forge::ForgeImpl,
    jj::Jujutsu,
    submit::{
        execute::{self, MRUpdate, MRUpdateType},
        plan,
    },
};

#[derive(Args)]
pub struct SubmitCommandConfig {
    /// Options for the revset
    #[command(flatten)]
    pub revset_options: SubmitCommandRevsetOptions,

    /// The remote to push to.
    #[arg(long, default_value = "origin")]
    pub remote: String,

    /// Don't actually modify any merge requests or push bookmarks, only print
    /// what would be done.
    #[arg(long)]
    pub dry_run: bool,
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
#[group(required = true, multiple = false)]
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

impl SubmitCommandRevsetOptions {
    fn to_get_bookmarks_options(&self) -> GetBookmarksOptions {
        match (
            self.revset_positional.as_deref(),
            self.revset.as_deref(),
            self.tracked,
        ) {
            (Some(revset), None, false) => GetBookmarksOptions::Revset(revset.to_string()),
            (None, Some(revset), false) => GetBookmarksOptions::Revset(revset.to_string()),
            (None, None, true) => GetBookmarksOptions::Tracked,
            _ => unreachable!(),
        }
    }
}

impl Default for SubmitCommandConfig {
    fn default() -> Self {
        Self {
            revset_options: Default::default(),
            remote: "origin".to_string(),
            dry_run: false,
        }
    }
}

/// Submit bookmarks and their dependencies as GitLab MRs
pub async fn submit(config: &SubmitCommandConfig, cli_config: &CliConfig<'_>) -> Result<()> {
    let jj = Jujutsu::new(&cli_config.repository)?;

    let revset = config.revset_options.to_get_bookmarks_options().to_revset();
    let changes = jj.log(revset)?;
    let bookmarks: Vec<_> = Bookmark::from_changes(&changes).into_iter().collect();

    ensure_whatever!(!bookmarks.is_empty(), "No bookmarks in revset");

    let output = cli_config.output;
    let repo_config = Config::load(&cli_config.repository)?;
    let forge = ForgeImpl::new(&repo_config)?;

    output.log_message(&format!(
        "Submitting bookmarks: {}",
        bookmarks
            .iter()
            .map(|b| b.name().magenta().to_string())
            .join(", ")
    ));

    let changes = jj.log(format!(
        "(({}) & mine() & bookmarks()) ~ trunk()",
        bookmarks
            .iter()
            .map(|b| format!("::{}", b.name()))
            .join(" | ")
    ))?;
    let bookmarks: Vec<_> = Bookmark::from_changes(&changes).into_iter().collect();
    let bookmark_graph = BookmarkGraph::from_bookmarks(
        &jj,
        bookmarks.iter().cloned(),
        config.revset_options.tracked,
    )?;

    let submission_plan = plan::plan(
        &jj,
        &forge,
        &repo_config,
        &bookmark_graph,
        config.dry_run,
        output,
    )
    .await?;

    let result = execute::execute(&submission_plan, &jj, &forge, &repo_config, output).await?;

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
        for (index, update) in updates.clone().into_iter().enumerate() {
            if let MRUpdateType::DescriptionUpdated = update.update_type
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
                MRUpdateType::Repointed { .. }
                | MRUpdateType::Both { .. }
                | MRUpdateType::DescriptionUpdated => {
                    table.push(vec![
                        bookmark.magenta().cell(),
                        mr.title().wrap(60).cell(),
                        mr.url().dimmed().cell(),
                        "[updated]".green().dimmed().cell(),
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
            errors: result.errors,
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
