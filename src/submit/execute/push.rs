use bon::Builder;
use owo_colors::OwoColorize;
use tracing::{debug, error};

use crate::{
    error::{Error, Result},
    submit::execute::{ActionInfo, ActionResultData, ExecuteAction, ExecuteActionContext},
};

/// Push a bookmark to a remote
#[derive(Debug, Clone, PartialEq, Eq, Builder)]
pub struct PushAction {
    pub bookmark: String,
    pub remote: String,
}

impl ActionInfo for PushAction {
    fn id(&self) -> String {
        format!("push:{}", self.bookmark)
    }

    fn group_text(&self) -> String {
        "Pushing bookmarks".to_string()
    }

    fn text(&self) -> String {
        format!("Pushing {}", self.bookmark.magenta())
    }

    fn substep_text(&self) -> String {
        self.bookmark.magenta().to_string()
    }

    fn plan_text(&self) -> String {
        format!(
            "Push bookmark {} to remote {}",
            self.bookmark.magenta(),
            self.remote.cyan()
        )
    }
}

impl ExecuteAction for PushAction {
    async fn execute(&self, ctx: ExecuteActionContext<'_>) -> Result<ActionResultData> {
        if ctx.execute.dry_run {
            ctx.execute.output.log_message(&format!(
                "Would push {} to {}",
                self.bookmark.magenta(),
                self.remote
            ));
            Ok(ActionResultData::Pushed {
                bookmark: self.bookmark.clone(),
                pushed: true,
            })
        } else {
            match ctx
                .execute
                .jj
                .push_bookmark(&self.bookmark, Some(&self.remote))
            {
                Ok(pushed) => {
                    if pushed {
                        ctx.execute
                            .output
                            .log_completed(&format!("Pushed {}", self.bookmark.magenta()));
                    } else {
                        debug!("Nothing needed to be pushed for {}", self.bookmark);
                    }
                    Ok(ActionResultData::Pushed {
                        bookmark: self.bookmark.clone(),
                        pushed,
                    })
                }
                Err(e) => {
                    let error_msg = format!("Failed to push {}: {}", self.bookmark, e);
                    ctx.execute.output.log_message(&error_msg);
                    error!("{}", error_msg);
                    Err(Error::new(error_msg))
                }
            }
        }
    }
}
