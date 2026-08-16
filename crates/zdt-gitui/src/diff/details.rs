//! What one commit is.

use zgui::prelude::*;
use zgui::{component, view};

use crate::labels::ago;

/// What one commit is.
#[component]
pub(crate) fn CommitDetails(
    /// Which commit.
    commit: zdt_git::Commit,
) -> impl IntoView {
    let body = commit.body.clone();

    view! {
        column(class = "git__details") {
            row(class = "git__details-line") {
                label(class = "git__short") {{commit.short.clone()}}
                label(class = "git__summary nowrap") {{commit.summary.clone()}}
            }
            row(class = "git__details-line muted") {
                label(class = "nowrap") {{format!("{} <{}>", commit.author, commit.email)}}
                box(class = "fill") {}
                label {{ago(commit.when)}}
            }
            {(!body.is_empty()).then(|| view! {
                box(class = "git__details-body") {{body}}
            })}
        }
    }
}
