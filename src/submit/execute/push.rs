use async_trait::async_trait;
use owo_colors::OwoColorize;
use tracing::{debug, error};

use crate::{
    error::{Error, Result},
    submit::execute::{ActionResultData, ExecuteAction, ExecutionActionContext},
};

pub struct PushAction {
    pub bookmark: String,
    pub remote: String,
}

impl PushAction {
    pub fn new(bookmark: String, remote: String) -> Self {
        Self { bookmark, remote }
    }
}

#[async_trait]
impl ExecuteAction for PushAction {
    async fn execute(&self, ctx: ExecutionActionContext<'_>) -> Result<ActionResultData> {
        if ctx.plan.dry_run {
            ctx.output.log_message(&format!(
                "Would push {} to {}",
                self.bookmark.magenta(),
                self.remote
            ));
            Ok(ActionResultData::Pushed {
                bookmark: self.bookmark.clone(),
                pushed: true,
            })
        } else {
            ctx.output
                .log_current(&format!("Pushing {}", self.bookmark.magenta()));

            match ctx.jj.push_bookmark(&self.bookmark, &self.remote) {
                Ok(pushed) => {
                    if pushed {
                        ctx.output
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
                    ctx.output.log_message(&error_msg);
                    error!("{}", error_msg);
                    Err(Error::new(error_msg))
                }
            }
        }
    }
}
