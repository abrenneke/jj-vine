use std::collections::HashMap;

use bon::Builder;
use itertools::Itertools;
use owo_colors::OwoColorize;
use tracing::{debug, error};

use crate::{
    error::{Error, Result},
    submit::execute::{ActionInfo, ActionResultData, ExecuteAction, ExecuteActionContext},
};

/// Push bookmarks to a remote
#[derive(Debug, Clone, PartialEq, Eq, Builder)]
pub struct PushAction {
    pub bookmarks: Vec<String>,

    pub remote: String,
}

impl ActionInfo for PushAction {
    fn id(&self) -> String {
        format!("push:{}", self.bookmarks.join(","))
    }

    fn group_text(&self) -> String {
        "Pushing bookmarks".to_string()
    }

    fn text(&self) -> String {
        format!(
            "Pushing {}",
            self.bookmarks.iter().map(|b| b.magenta()).join(", ")
        )
    }

    fn substep_text(&self) -> String {
        self.bookmarks.iter().map(|b| b.magenta()).join(", ")
    }

    fn plan_text(&self) -> String {
        format!(
            "Push bookmarks to remote {}: {}",
            self.remote.cyan(),
            self.bookmarks.iter().map(|b| b.magenta()).join(", "),
        )
    }
}

impl ExecuteAction for PushAction {
    fn execute(
        &self,
        ctx: ExecuteActionContext<'_>,
    ) -> impl Future<Output = Result<ActionResultData>> {
        let bookmarks_string = self.bookmarks.iter().map(|b| b.magenta()).join(", ");
        std::future::ready(if ctx.execute.dry_run {
            ctx.execute.output.log_message(&format!(
                "Would push bookmarks to remote {}: {bookmarks_string}",
                self.remote.cyan()
            ));

            Ok(ActionResultData::Pushed {
                bookmarks: self.bookmarks.clone(),
                created_bookmarks: HashMap::new(),
                pushed: true,
            })
        } else {
            match ctx
                .execute
                .jj
                .push_bookmarks(&self.bookmarks, Some(&self.remote))
            {
                Ok(pushed) => {
                    if pushed {
                        ctx.execute
                            .output
                            .log_completed(&format!("Pushed {bookmarks_string}"));
                    } else {
                        debug!("Nothing needed to be pushed for {bookmarks_string}");
                    }

                    Ok(ActionResultData::Pushed {
                        bookmarks: self.bookmarks.clone(),
                        created_bookmarks: HashMap::new(),
                        pushed,
                    })
                }
                Err(e) => {
                    let error_msg = format!("Failed to push {bookmarks_string}: {e}");
                    ctx.execute.output.log_message(&error_msg);
                    error!("{error_msg}");
                    Err(Error::new(error_msg))
                }
            }
        })
    }
}
