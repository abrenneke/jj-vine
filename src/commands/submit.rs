use std::borrow::Cow;

use clap::Args;
use cli_table::{
    Cell,
    Table,
    format::{Border, Separator},
};
use itertools::Itertools;
use owo_colors::OwoColorize;
use snafu::ensure;
use tracing::info;
use unicode_segmentation::UnicodeSegmentation;

use crate::{
    bookmark::{Bookmark, BookmarkGraph},
    cli::CliConfig,
    commands::{GetBookmarksOptions, StrVisualWidth, get_changes_from_cli_args},
    config::Config,
    error::{CLISnafu, Error, Result},
    forge::create_forge,
    jj::Jujutsu,
    submit::{
        execute::{self, MRUpdate, MRUpdateType},
        plan,
    },
};

#[derive(Args)]
pub struct SubmitCommandConfig {
    /// The revset to submit (mutually exclusive with --tracked)
    pub revset: Option<String>,

    /// Submit all tracked bookmarks (equivalent to "(mine() &
    /// tracked_remote_bookmarks()) ~ trunk()")
    #[arg(long)]
    pub tracked: bool,

    /// Remote to push to
    #[arg(long, default_value = "origin")]
    pub remote: String,

    /// Dry run - don't actually push or create MRs
    #[arg(long)]
    pub dry_run: bool,
}

impl Default for SubmitCommandConfig {
    fn default() -> Self {
        Self {
            revset: None,
            tracked: false,
            remote: "origin".to_string(),
            dry_run: false,
        }
    }
}

impl SubmitCommandConfig {
    fn to_get_bookmarks_options(&self) -> GetBookmarksOptions {
        GetBookmarksOptions {
            mine: false,
            revset: self.revset.clone(),
            tracked: self.tracked,
        }
    }
}

/// Submit bookmarks and their dependencies as GitLab MRs
pub async fn submit(config: SubmitCommandConfig, cli_config: CliConfig<'_>) -> Result<()> {
    let jj = Jujutsu::new(&cli_config.repository)?;

    let changes = get_changes_from_cli_args(&config.to_get_bookmarks_options(), &jj)?;
    let bookmarks: Vec<_> = Bookmark::from_changes(&changes).into_iter().collect();

    ensure!(
        !bookmarks.is_empty(),
        CLISnafu {
            message: "No bookmarks in revset".to_string(),
        }
    );

    let output = cli_config.output;
    let repo_config = Config::load(&cli_config.repository)?;
    let forge = create_forge(&repo_config)?;

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
    let bookmark_graph =
        BookmarkGraph::from_bookmarks(&jj, bookmarks.iter().cloned(), config.tracked)?;

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

        for MRUpdate {
            mr,
            bookmark,
            update_type,
        } in result.merge_requests.iter().sorted_by_key(|mr| mr.mr.iid())
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
        return Err(Error::Config {
            message: format!(
                "{} error(s) occurred during submission",
                result.errors.len()
            ),
        });
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
