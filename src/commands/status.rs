use std::cmp::Ordering;

use clap::Args;
use futures::{StreamExt, stream::FuturesUnordered};
use itertools::Itertools;
use jiff::SpanRound;
use owo_colors::OwoColorize;
use pluralizer::pluralize;
use tracing::info;

use crate::{
    cli::CliConfig,
    config::Config,
    description::FormatMergeRequest,
    error::{Error, Result},
    forge::{
        ApprovalSatisfaction,
        CheckStatus,
        ForgeMergeRequest,
        MergeRequestStatus,
        create_forge,
    },
    jj::Jujutsu,
};

#[derive(Args)]
pub struct StatusCommandConfig {
    /// Output mode
    /// - flat: Flat list of bookmarks and their status
    #[arg(short = 'o', long = "output", default_value = "flat")]
    output_mode: DisplayStatusMode,
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

pub async fn status(config: StatusCommandConfig, cli_config: CliConfig<'_>) -> Result<()> {
    let jj = Jujutsu::new(cli_config.repository.clone())?;
    let repo_config = Config::load(&cli_config.repository)?;
    let forge = create_forge(&repo_config)?;
    let output = cli_config.output;

    output.log_current("Finding tracked bookmarks");

    let tracked_bookmarks = jj.get_tracked_bookmarks(&repo_config.remote_name)?;

    if tracked_bookmarks.is_empty() {
        output.finish();
        info!("No tracked bookmarks found.");
        return Ok(());
    }

    output.log_current("Checking status of tracked bookmarks");

    let futures = FuturesUnordered::new();

    for bookmark in &tracked_bookmarks {
        futures.push(async {
            let _substep = output.start_substep(bookmark.clone());

            let mr_option = forge
                .find_merge_request_by_source_branch(bookmark)
                .await
                .map_err(|e| BookmarkStatusError {
                    bookmark: bookmark.clone(),
                    error: e,
                })?;

            let status = match mr_option {
                Some(mr) => {
                    let mr_status = forge
                        .get_merge_request_status(mr.iid().as_ref())
                        .await
                        .map_err(|e| BookmarkStatusError {
                            bookmark: bookmark.clone(),
                            error: e,
                        })?;
                    BookmarkStatus::HasMergeRequest {
                        bookmark: bookmark.clone(),
                        merge_request: Box::new(mr),
                        status: mr_status,
                    }
                }
                None => BookmarkStatus::NoMergeRequest {
                    bookmark: bookmark.clone(),
                },
            };

            Ok(status)
        });
    }

    let statuses = futures.collect::<Vec<_>>().await;

    output.finish();

    config.output_mode.print(statuses, forge.as_ref());

    Ok(())
}

#[derive(Clone, Copy)]
enum DisplayStatusMode {
    FlatList,
}

impl std::str::FromStr for DisplayStatusMode {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "flat" => Ok(DisplayStatusMode::FlatList),
            _ => Err(Error::Config {
                message: format!("Invalid output mode: {}. Valid modes are: flat", s),
            }),
        }
    }
}

impl DisplayStatusMode {
    fn print(
        &self,
        statuses: Vec<Result<BookmarkStatus, BookmarkStatusError>>,
        format_merge_request_id: &dyn FormatMergeRequest,
    ) {
        match self {
            DisplayStatusMode::FlatList => {
                print_two_line_compact(statuses, format_merge_request_id)
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

/// Prints the status of a merge request in a two-line compact format.
/// Example:
/// !123 MR Title Here
///      my-bookmark • [READY] • ✓ Checks OK • Approved (3/3)
fn print_two_line_compact(
    statuses: Vec<Result<BookmarkStatus, BookmarkStatusError>>,
    format_merge_request_id: &dyn FormatMergeRequest,
) {
    for status in sorted_statuses(&statuses) {
        match status {
            Ok(BookmarkStatus::HasMergeRequest {
                bookmark,
                merge_request,
                status,
            }) => {
                let iid = format_merge_request_id.format_merge_request_id(&status.iid);

                let parts = [
                    // Bookmark name
                    Some(bookmark.magenta().to_string()),
                    // Ready to merge
                    if status.ready_to_merge() {
                        Some("[READY]".green().bold().to_string())
                    } else {
                        None
                    },
                    // Checks status
                    match status.check_status {
                        CheckStatus::Success => Some("✓ Checks OK".green().to_string()),
                        CheckStatus::Failed => Some("✗ Checks failing".red().to_string()),
                        CheckStatus::Pending => Some("⋯ Checks pending".yellow().to_string()),
                        CheckStatus::None => None,
                    },
                    // Approval status
                    if status.approval_status.required_count == 0 {
                        Some(
                            pluralize(
                                "approval",
                                status.approval_status.approved_count as isize,
                                true,
                            )
                            .white()
                            .to_string(),
                        )
                    } else if status.approval_status.satisfaction == ApprovalSatisfaction::Satisfied
                    {
                        Some(
                            format!(
                                "Approved ({}/{})",
                                status.approval_status.approved_count,
                                status.approval_status.required_count
                            )
                            .green()
                            .to_string(),
                        )
                    } else {
                        Some(
                            format!(
                                "Needs {} ({}/{})",
                                pluralize(
                                    "approval",
                                    status.approval_status.required_count as isize,
                                    false
                                ),
                                status.approval_status.approved_count,
                                status.approval_status.required_count
                            )
                            .red()
                            .to_string(),
                        )
                    },
                    // Created at
                    {
                        let now = jiff::Zoned::now();
                        let duration = (merge_request.created_at() - now.timestamp()).abs();

                        let mut round = SpanRound::new().largest(jiff::Unit::Year).relative(&now);

                        if duration.total((jiff::Unit::Hour, &now)).unwrap() > 24.0 {
                            round = round.smallest(jiff::Unit::Hour);
                        } else if duration.total((jiff::Unit::Minute, &now)).unwrap() > 0.0 {
                            round = round.smallest(jiff::Unit::Minute);
                        } else {
                            round = round.smallest(jiff::Unit::Second);
                        }

                        Some(
                            format!(
                                "{:#} old",
                                duration.round(round).expect("Failed to round duration")
                            )
                            .dimmed()
                            .to_string(),
                        )
                    },
                    // URL
                    Some(merge_request.url().truecolor(100, 100, 100).to_string()),
                ];

                info!(
                    "{} {}\n{}{}\n",
                    iid.cyan(),
                    merge_request.title().white(),
                    " ".repeat(iid.len() + 1),
                    parts
                        .into_iter()
                        .flatten()
                        .join(&" • ".dimmed().to_string())
                );
            }
            Ok(BookmarkStatus::NoMergeRequest { bookmark }) => {
                info!("{} {}", bookmark.magenta(), "No merge request".dimmed());
            }
            Err(BookmarkStatusError { bookmark, error }) => {
                info!(
                    "Failed to get status for bookmark {}: {}",
                    bookmark.magenta(),
                    error
                );
            }
        }
    }
}
