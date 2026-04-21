use bon::Builder;
use itertools::Itertools;
use owo_colors::OwoColorize;
use tracing::error;

use crate::{
    bookmark::{Bookmark, change_id_to_temp_bookmark_name},
    error::{Error, Result},
    submit::execute::{ActionInfo, ActionResultData, ExecuteAction, ExecuteActionContext},
};

/// Push changes to a remote using -c
#[derive(Debug, Clone, PartialEq, Eq, Builder)]
pub struct PushCreateAction {
    pub change_ids: Vec<String>,

    pub remote: String,
}

impl ActionInfo for PushCreateAction {
    fn id(&self) -> String {
        format!("push_create:{}", self.change_ids.join(","))
    }

    fn group_text(&self) -> String {
        "Creating and pushing bookmarks".to_string()
    }

    fn text(&self) -> String {
        format!(
            "Creating and pushing {}",
            self.change_ids
                .iter()
                .map(|c| (&c[..8]).magenta().to_string())
                .join(", ")
        )
    }

    fn substep_text(&self) -> String {
        self.change_ids
            .iter()
            .map(|c| (&c[..8]).magenta().to_string())
            .join(", ")
    }

    fn plan_text(&self) -> String {
        format!(
            "Create and push bookmarks to remote {} for changes: {}",
            self.remote.cyan(),
            self.change_ids
                .iter()
                .map(|c| (&c[..8]).magenta().to_string())
                .join(", "),
        )
    }
}

impl ExecuteAction for PushCreateAction {
    async fn execute(&self, ctx: ExecuteActionContext<'_>) -> Result<ActionResultData> {
        let change_ids_string = self.change_ids.iter().map(|b| b.magenta()).join(", ");

        if ctx.execute.dry_run {
            ctx.execute.output.log_message(&format!(
                "Would create bookmarks and push to remote {} for changes: {change_ids_string}",
                self.remote.cyan()
            ));

            Ok(ActionResultData::Pushed {
                bookmarks: self
                    .change_ids
                    .iter()
                    .map(|c| change_id_to_temp_bookmark_name(c))
                    .collect(),
                created_bookmarks: self
                    .change_ids
                    .iter()
                    .map(|c| (c.clone(), change_id_to_temp_bookmark_name(c)))
                    .collect(),
                pushed: true,
            })
        } else {
            match ctx
                .execute
                .jj
                .push_changes_create(&self.change_ids, Some(&self.remote))
            {
                Ok(_) => {
                    let changes = ctx.execute.jj.log(self.change_ids.join("|"))?;
                    let bookmarks: Vec<_> = Bookmark::from_changes(&changes).into_iter().collect();

                    ctx.execute.output.log_completed(&format!(
                        "Created bookmarks: {}",
                        bookmarks
                            .iter()
                            .map(|b| b.name().magenta().to_string())
                            .join(", ")
                    ));

                    Ok(ActionResultData::Pushed {
                        bookmarks: bookmarks.iter().map(|b| b.name().to_string()).collect(),
                        created_bookmarks: bookmarks
                            .iter()
                            .map(|b| (b.change.change_id.clone(), b.name().to_string()))
                            .collect(),
                        pushed: true,
                    })
                }
                Err(e) => {
                    let error_msg = format!(
                        "Failed to create and push changes to remote {} ({change_ids_string}): {e}",
                        self.remote.cyan()
                    );
                    ctx.execute.output.log_message(&error_msg);
                    error!("{error_msg}");
                    Err(Error::new(error_msg))
                }
            }
        }
    }
}
