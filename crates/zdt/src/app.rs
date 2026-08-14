//! What the window contains.
//!
//! Three rows inside the frame: the combined header, the panes, and the status line. Everything
//! below reads the workspace, which is provided here and nowhere else.

use std::path::PathBuf;

use zdt_core::{Project, ThemeSource};
use zgui::prelude::*;
use zgui::reactive::RwSignal;
use zgui::{component, view};
use zgui_ui_tokens::ColorScheme;

use crate::ui::chrome::ChromeProps;
use crate::ui::frame::FrameProps;
use crate::ui::panes::PanesProps;
use crate::ui::statusline::StatusLineProps;
use crate::ui::whichkey::WhichKeyProps;
use crate::ui::theme::{ZdtThemeProps, fallback};
use crate::workspace::{self, Workspace};

/// The application.
#[component]
pub fn Root(
    /// The directory the editor was opened on.
    project: Project,
    /// The files named on the command line.
    files: Vec<PathBuf>,
) -> impl IntoView {
    // Both are the settings file's to write, once there is one. Until then they are what the
    // interface starts as, and the type they are held in is already the one a configuration
    // change will write into.
    let theme: RwSignal<ThemeSource, zgui::reactive::LocalStorage> =
        RwSignal::new_local(fallback());
    let scheme = RwSignal::new_local(ColorScheme::Dark);

    let space = Workspace::new(project);
    workspace::provide(space.clone());
    zgui::reactive::provide_local_context(crate::vim::Vim::new(space.clone()));

    for file in files {
        crate::files::open_argument(&space, &file);
    }

    view! {
        ZdtTheme(theme = theme, scheme = scheme) {
            Frame {
                Chrome()
                Panes()
                WhichKey()
                StatusLine()
            }
        }
    }
}
