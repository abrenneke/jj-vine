use std::cmp::Ordering;

use clap::{Args, ValueEnum};
use enum_dispatch::enum_dispatch;
use futures::{
    StreamExt,
    stream::{FuturesOrdered, FuturesUnordered},
    try_join,
};
use itertools::Itertools;
use jiff::SpanRound;
use owo_colors::OwoColorize;
use pluralizer::pluralize;
use tracing::info;

use crate::{
    bookmark::Bookmark,
    cli::CliConfig,
    commands::{GetBookmarksOptions, StrVisualWidth},
    config::Config,
    description::FormatMergeRequest,
    error::{ConfigSnafu, Error, InvalidComponentSnafu, Result},
    forge::{
        ApprovalSatisfaction,
        CheckStatus,
        Forge,
        ForgeImpl,
        ForgeMergeRequest,
        MergeRequestStatus,
    },
    jj::Jujutsu,
    output::Output,
};

#[derive(Args)]
pub struct StatusCommandConfig {
    /// Output mode
    /// - two-line-compact: Two-lines per merge request
    #[arg(short = 'o', long = "output", default_value = "two-line-compact")]
    pub output_mode: DisplayStatusMode,

    /// Options for the revset
    #[command(flatten)]
    pub revset_options: StatusCommandRevsetOptions,
}

impl StatusCommandConfig {
    pub fn help_long() -> String {
        format!(
            r#"
Show the status of tracked bookmarks and their {}

{}

Show the status of all my bookmarks:
{}

Show the status of all tracked bookmarks:
{}

Show the status of a specific revset:
{}
"#,
            match ForgeImpl::from_cwd() {
                Ok(forge) => format!("{}s", forge.mr_name()),
                Err(_) => "MRs/PRs".to_string(),
            },
            "Examples:".yellow().bold(),
            "jj vine status".green().bold(),
            "jj vine status --tracked".green().bold(),
            "jj vine status -r <revset>".green().bold(),
        )
    }
}

#[derive(Args, Default)]
#[group(required = false, multiple = false)]
pub struct StatusCommandRevsetOptions {
    /// Use a manual revset
    #[arg(short = 'r', long)]
    pub revset: Option<String>,

    /// Include only `(mine() & tracked_remote_bookmarks()) ~ trunk()`
    #[arg(long)]
    pub tracked: bool,
}

impl StatusCommandRevsetOptions {
    fn to_get_bookmarks_options(&self) -> GetBookmarksOptions {
        match (self.revset.as_deref(), self.tracked) {
            (Some(revset), false) => GetBookmarksOptions::Revset(revset.to_string()),
            (None, true) => GetBookmarksOptions::Tracked,
            (None, false) => GetBookmarksOptions::Mine,
            _ => unreachable!(),
        }
    }
}

enum BookmarkStatus {
    HasMergeRequest {
        bookmark: String,
        merge_request: Box<ForgeMergeRequest>,
        status: MergeRequestStatus,
    },
    NoMergeRequest {
        bookmark: String,
    },
}

struct BookmarkStatusError {
    bookmark: String,
    error: Error,
}

pub async fn status(config: &StatusCommandConfig, cli_config: &CliConfig<'_>) -> Result<()> {
    let jj = Jujutsu::new(&cli_config.repository)?;
    let repo_config = Config::load(&cli_config.repository)?;
    let forge = ForgeImpl::new(&repo_config)?;
    let output = cli_config.output;

    let revset = config.revset_options.to_get_bookmarks_options().to_revset();
    let changes = jj.log(revset)?;
    let bookmarks: Vec<_> = Bookmark::from_changes(&changes).into_iter().collect();

    if bookmarks.is_empty() {
        output.finish();
        info!("No tracked bookmarks found.");
        return Ok(());
    }

    output.log_current("Checking status of tracked bookmarks");

    let futures = FuturesUnordered::new();

    for bookmark in &bookmarks {
        futures.push(async {
            let _substep = output.start_substep(bookmark.name().to_string());

            let mr_option = forge
                .find_merge_request_by_source_branch(bookmark.name())
                .await
                .map_err(|e| BookmarkStatusError {
                    bookmark: bookmark.name().to_string(),
                    error: e,
                })?;

            let status = match mr_option {
                Some(mr) => {
                    let mr_status = forge
                        .get_merge_request_status(mr.iid().as_ref())
                        .await
                        .map_err(|e| BookmarkStatusError {
                            bookmark: bookmark.name().to_string(),
                            error: e,
                        })?;
                    BookmarkStatus::HasMergeRequest {
                        bookmark: bookmark.name().to_string(),
                        merge_request: Box::new(mr),
                        status: mr_status,
                    }
                }
                None => BookmarkStatus::NoMergeRequest {
                    bookmark: bookmark.name().to_string(),
                },
            };

            Ok(status)
        });
    }

    let statuses = futures.collect::<Vec<_>>().await;

    let rendered = config.output_mode.render(statuses, &forge, output).await;

    output.finish();

    info!("{}", rendered);

    Ok(())
}

#[derive(ValueEnum, Clone, Copy)]
pub enum DisplayStatusMode {
    /// Render the status of merge requests in a two-line compact format.
    #[value(id = "two-line-compact")]
    TwoLineCompact,
}

impl std::str::FromStr for DisplayStatusMode {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "flat" => Ok(DisplayStatusMode::TwoLineCompact),
            _ => Err(ConfigSnafu {
                message: format!("Invalid output mode: {}. Valid modes are: flat", s),
            }
            .build()),
        }
    }
}

impl DisplayStatusMode {
    async fn render(
        &self,
        statuses: Vec<Result<BookmarkStatus, BookmarkStatusError>>,
        forge: &ForgeImpl,
        output: &dyn Output,
    ) -> String {
        match self {
            DisplayStatusMode::TwoLineCompact => {
                print_two_line_compact(statuses, forge, output).await
            }
        }
    }
}

fn sorted_statuses(
    statuses: &[Result<BookmarkStatus, BookmarkStatusError>],
) -> impl Iterator<Item = &Result<BookmarkStatus, BookmarkStatusError>> + '_ {
    statuses.iter().sorted_by(|a, b| match (a, b) {
        // Order by merge request ID first
        (
            Ok(BookmarkStatus::HasMergeRequest {
                merge_request: a, ..
            }),
            Ok(BookmarkStatus::HasMergeRequest {
                merge_request: b, ..
            }),
        ) => a.iid().cmp(&b.iid()),

        // Place merge requests before no merge requests
        (Ok(BookmarkStatus::HasMergeRequest { .. }), _) => Ordering::Less,
        (_, Ok(BookmarkStatus::HasMergeRequest { .. })) => Ordering::Greater,

        // Order no merge requests by bookmark name
        (
            Ok(BookmarkStatus::NoMergeRequest { bookmark: a }),
            Ok(BookmarkStatus::NoMergeRequest { bookmark: b }),
        ) => a.cmp(b),

        // Place errors last
        (Ok(BookmarkStatus::NoMergeRequest { .. }), _) => Ordering::Less,
        (_, Ok(BookmarkStatus::NoMergeRequest { .. })) => Ordering::Greater,
        (Err(_), Err(_)) => Ordering::Equal,
    })
}

/// Renders the status of merge requests in a two-line compact format.
/// Example:
/// !123 MR Title Here
///      my-bookmark • [READY] • ✓ Checks OK • Approved (3/3)
async fn print_two_line_compact(
    statuses: Vec<Result<BookmarkStatus, BookmarkStatusError>>,
    forge: &ForgeImpl,
    output: &dyn Output,
) -> String {
    let futures: FuturesOrdered<_> = sorted_statuses(&statuses)
        .map(|status| two_line_compact_status(status, forge, output))
        .collect();

    futures
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .flatten()
        .join("\n\n")
}

async fn two_line_compact_status(
    status: &Result<BookmarkStatus, BookmarkStatusError>,
    forge: &ForgeImpl,
    output: &dyn Output,
) -> Option<String> {
    match status {
        Ok(BookmarkStatus::HasMergeRequest {
            bookmark,
            merge_request,
            status,
        }) => {
            let _substep =
                output.start_substep(forge.format_merge_request_id(&merge_request.iid()));

            let data = StatusData::new(
                forge,
                bookmark.clone(),
                merge_request.clone(),
                status.clone(),
            );

            // TODO would love templating language like jj has
            let first_line = match parse_components("iid title") {
                Ok(line) => line,
                Err(e) => return Some(e.to_string()),
            };
            let second_line = match parse_components(
                "bookmark ready checks approval num_discussions created url",
            ) {
                Ok(line) => line,
                Err(e) => return Some(e.to_string()),
            };

            let (first_line, second_line) = match try_join!(
                render_components(first_line, &data),
                render_components(second_line, &data)
            ) {
                Ok(lines) => lines,
                Err(err) => return Some(err.to_string()),
            };

            let second_line_padding =
                " ".repeat(first_line.first().unwrap_or(&"".to_string()).visual_width() + 1);

            Some(format!(
                "{}\n{}{}",
                first_line.join(" "),
                second_line_padding,
                second_line.join(&" • ".dimmed().to_string()),
            ))
        }
        Ok(BookmarkStatus::NoMergeRequest { bookmark }) => Some(format!(
            "{} {}",
            bookmark.magenta(),
            "No merge request".dimmed()
        )),
        Err(BookmarkStatusError { bookmark, error }) => Some(format!(
            "Failed to get status for bookmark {}: {}",
            bookmark.magenta(),
            error
        )),
    }
}

fn parse_components(line: &str) -> Result<Vec<StatusComponentImpl>> {
    line.split_whitespace()
        .map(get_component)
        .collect::<Result<Vec<_>>>()
}

async fn render_components(
    components: Vec<StatusComponentImpl>,
    data: &StatusData<'_>,
) -> Result<Vec<String>> {
    let futures: FuturesOrdered<_> = components
        .iter()
        .map(|component| component.render(data))
        .collect();

    Ok(futures
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect())
}

fn get_component(name: &str) -> Result<StatusComponentImpl> {
    match name {
        "bookmark" => Ok(BookmarkNameComponent {}.into()),
        "iid" => Ok(MergeRequestIIDComponent {}.into()),
        "title" => Ok(MergeRequestTitleComponent {}.into()),
        "ready" => Ok(ReadyToMergeComponent {}.into()),
        "checks" => Ok(ChecksStatusComponent {}.into()),
        "approval" => Ok(ApprovalStatusComponent {}.into()),
        "created" => Ok(CreatedAtComponent {}.into()),
        "url" => Ok(MergeRequestURLComponent {}.into()),
        "num_discussions" => Ok(NumOpenDiscussionsComponent {}.into()),
        _ => Err(InvalidComponentSnafu {
            component: name.to_string(),
        }
        .build()),
    }
}

#[enum_dispatch]
trait StatusComponent: Send + Sync {
    async fn render(&self, status: &StatusData<'_>) -> Result<Option<String>>;
}

#[enum_dispatch(StatusComponent)]
enum StatusComponentImpl {
    BookmarkName(BookmarkNameComponent),
    MergeRequestIID(MergeRequestIIDComponent),
    MergeRequestTitle(MergeRequestTitleComponent),
    ReadyToMerge(ReadyToMergeComponent),
    ChecksStatus(ChecksStatusComponent),
    ApprovalStatus(ApprovalStatusComponent),
    CreatedAt(CreatedAtComponent),
    MergeRequestURL(MergeRequestURLComponent),
    NumOpenDiscussions(NumOpenDiscussionsComponent),
}

macro_rules! component {
    ($struct_name:ident, $body:expr) => {
        struct $struct_name {}

        impl StatusComponent for $struct_name {
            async fn render(&self, status: &StatusData<'_>) -> Result<Option<String>> {
                ($body)(status).await
            }
        }
    };
}

struct StatusData<'a> {
    forge: &'a ForgeImpl,
    bookmark: String,
    merge_request: Box<ForgeMergeRequest>,
    status: MergeRequestStatus,
}

impl<'a> StatusData<'a> {
    fn new(
        forge: &'a ForgeImpl,
        bookmark: String,
        merge_request: Box<ForgeMergeRequest>,
        status: MergeRequestStatus,
    ) -> Self {
        Self {
            forge,
            bookmark,
            merge_request,
            status,
        }
    }
}

component!(BookmarkNameComponent, async |data: &StatusData<'_>| {
    Ok(Some(data.bookmark.magenta().to_string()))
});

component!(MergeRequestIIDComponent, async |data: &StatusData<'_>| {
    Ok(Some(
        data.forge
            .format_merge_request_id(&data.merge_request.iid())
            .cyan()
            .to_string(),
    ))
});

component!(MergeRequestTitleComponent, async |data: &StatusData<'_>| {
    Ok(Some(format!("\"{}\"", data.merge_request.title().white())))
});

component!(ReadyToMergeComponent, async |data: &StatusData<'_>| {
    if data.status.ready_to_merge() {
        Ok(Some("[READY]".green().bold().to_string()))
    } else {
        Ok(None)
    }
});

component!(ChecksStatusComponent, async |data: &StatusData<'_>| {
    match data.status.check_status {
        CheckStatus::Success => Ok(Some("✓ Checks OK".green().to_string())),
        CheckStatus::Failed => Ok(Some("✗ Checks failing".red().to_string())),
        CheckStatus::Pending => Ok(Some("⋯ Checks pending".yellow().to_string())),
        CheckStatus::None => Ok(None),
    }
});

component!(ApprovalStatusComponent, async |data: &StatusData<'_>| {
    let approval_status = &data.status.approval_status;
    if approval_status.required_count == 0
        && approval_status.satisfaction == ApprovalSatisfaction::Satisfied
    {
        Ok(Some(
            pluralize("approval", approval_status.approved_count as isize, true)
                .white()
                .to_string(),
        ))
    } else if approval_status.satisfaction == ApprovalSatisfaction::Satisfied {
        Ok(Some(
            format!(
                "Approved ({}/{})",
                approval_status.approved_count, approval_status.required_count
            )
            .green()
            .to_string(),
        ))
    } else if approval_status.satisfaction != ApprovalSatisfaction::Unknown {
        Ok(Some(
            format!(
                "Needs {} ({}/{})",
                pluralize("approval", approval_status.required_count as isize, false),
                approval_status.approved_count,
                approval_status.required_count
            )
            .red()
            .to_string(),
        ))
    } else {
        Ok(None)
    }
});

component!(CreatedAtComponent, async |data: &StatusData<'_>| {
    let now = jiff::Zoned::now();
    let duration = (data.merge_request.created_at() - now.timestamp()).abs();

    let mut round = SpanRound::new().largest(jiff::Unit::Year).relative(&now);

    if duration.total((jiff::Unit::Hour, &now)).unwrap() > 24.0 {
        round = round.smallest(jiff::Unit::Hour);
    } else if duration.total((jiff::Unit::Minute, &now)).unwrap() > 0.0 {
        round = round.smallest(jiff::Unit::Minute);
    } else {
        round = round.smallest(jiff::Unit::Second);
    }

    Ok(Some(
        format!(
            "{:#} old",
            duration.round(round).expect("Failed to round duration")
        )
        .dimmed()
        .to_string(),
    ))
});

component!(MergeRequestURLComponent, async |data: &StatusData<'_>| {
    Ok(Some(
        data.merge_request
            .url(data.forge)
            .truecolor(100, 100, 100)
            .to_string(),
    ))
});

component!(NumOpenDiscussionsComponent, async |data: &StatusData<
    '_,
>|
       -> Result<
    Option<String>,
> {
    let num_open_discussions = data
        .forge
        .num_open_discussions(&data.merge_request.iid())
        .await?;

    match num_open_discussions.unresolved {
        0 => Ok(None),
        unresolved => Ok(Some(
            format!(
                "{} open {}",
                unresolved,
                pluralize("discussion", unresolved as isize, false)
            )
            .yellow()
            .to_string(),
        )),
    }
});
