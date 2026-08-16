//! The status half: what is not staged, then what is.

use zgui::prelude::*;
use zgui::{component, view};

use crate::panel::List;

use crate::status::list::FileListProps;

pub(crate) use crate::status::row::FileRowProps;

mod list;
mod row;

/// The status half: what is not staged, then what is.
#[component]
pub(crate) fn StatusList() -> impl IntoView {
    view! {
        FileList(which = List::Unstaged, heading = "Changes")
        FileList(which = List::Staged, heading = "Staged")
    }
}
