//! The pickers.
//!
//! One modal, several sources, and a rule that the interface thread never waits for any of them.
//!
//! # What happens between a keystroke and a row
//!
//! A standing source is one like files, buffers or themes. Its candidates are gathered once when
//! the picker opens, and every keystroke after that re-ranks what is already in memory. The
//! ranking for the file list happens on `nucleo`'s own threads. This polls it a few times a second
//! from a timer, and stops as soon as it says it has settled. Nothing on the interface thread ever
//! walks the candidate list.
//!
//! A live source is grep, where the query *is* the search. Each keystroke cancels the search
//! before it and starts another after a short pause. Hits arrive in batches, so the first ones are
//! on the screen long before the walk has finished.
//!
//! A generation counter guards both. An answer that arrives for a query nobody is asking any more
//! is dropped.

pub mod view;

mod gather;
mod grep;
mod lists;
mod modal;
mod publish;
mod read;

pub mod source;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use zdt_core::search::fuzzy::Ranker;
use zdt_core::search::{Cancel, Walk};
use zgui::reactive::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};

pub use crate::picker::source::{
    Deed, Preview, Reach, Row, Source, Target, location_rows, symbol_rows,
};
use crate::settings::Settings;
use crate::workspace::Workspace;

/// How long after the last keystroke a grep starts.
///
/// Long enough that typing a word does not start six searches, short enough that it feels like
/// none was waited for.
const GREP_DEBOUNCE: Duration = Duration::from_millis(90);

/// How often the file matcher is asked whether it has anything new.
const POLL: Duration = Duration::from_millis(16);

/// The picker.
#[derive(Clone)]
pub struct Picker {
    inner: Rc<Inner>,
}

struct Inner {
    workspace: Workspace,
    settings: Settings,
    /// The window's clock, taken once at construction.
    ///
    /// Taken here and never where it is used. A timer started from inside a task's continuation,
    /// or from inside another timer's callback, sits outside the scope that has one, and asking
    /// there answers nothing. Taking it inside the root, where there certainly is one, is what
    /// makes the polling work wherever it is started from.
    timers: Option<zgui::view::time::Timers>,

    /// Which picker is open, if any.
    source: RwSignal<Option<Source>, LocalStorage>,
    /// What has been typed.
    query: RwSignal<String, LocalStorage>,
    /// What to show.
    rows: RwSignal<Vec<Row>, LocalStorage>,
    /// Which row the caret is on.
    at: RwSignal<usize, LocalStorage>,
    /// How many matched, and how many there are.
    counts: RwSignal<(usize, usize), LocalStorage>,
    /// Whether anything is still being gathered or searched.
    working: RwSignal<bool, LocalStorage>,

    /// Which question is being answered. An answer for an older one is thrown away.
    generation: Cell<u64>,
    /// Whether what is shown belongs to a question nobody is asking any more.
    ///
    /// The rows of the last search, kept on screen until the new one has something to replace
    /// them with. The first batch to arrive clears this and takes their place.
    stale: Cell<bool>,
    /// The candidates of a standing source, before ranking.
    candidates: RefCell<Vec<Row>>,
    /// The matcher, for the file list.
    ranker: RefCell<Option<Ranker>>,
    /// What is polling it, held so that dropping it stops the polling.
    polling: RefCell<Option<zgui::view::time::IntervalHandle>>,
    /// What is waiting to start a grep, held so that a newer keystroke cancels it.
    pending: RefCell<Option<zgui::view::time::TimeoutHandle>>,
    /// How to stop the grep that is running.
    cancel: RefCell<Option<Cancel>>,
    /// What a preview changed, and what to put back when it is given up on.
    ///
    /// Choosing a theme by reading its name is not choosing a theme; the way to know is to see
    /// it. So the caret moving applies it, and `<Esc>` puts back whatever was in force before the
    /// picker opened.
    restore: RefCell<Option<String>>,
}

impl Picker {
    /// A picker with nothing open.
    #[must_use]
    pub fn new(workspace: Workspace, settings: Settings) -> Self {
        Self {
            inner: Rc::new(Inner {
                workspace,
                settings,
                timers: zgui::view::time::Timers::current(),
                source: RwSignal::new_local(None),
                query: RwSignal::new_local(String::new()),
                rows: RwSignal::new_local(Vec::new()),
                at: RwSignal::new_local(0),
                counts: RwSignal::new_local((0, 0)),
                working: RwSignal::new_local(false),
                generation: Cell::new(0),
                stale: Cell::new(false),
                candidates: RefCell::new(Vec::new()),
                ranker: RefCell::new(None),
                polling: RefCell::new(None),
                pending: RefCell::new(None),
                cancel: RefCell::new(None),
                restore: RefCell::new(None),
            }),
        }
    }
}

/// What `git ls-files` says, or nothing when this is not a repository.
///
/// Blocking, and a process. The answer is a list of names that git already knows, and linking a
/// git implementation to ask would be a large dependency for one list.
fn git_files(root: &std::path::Path) -> Vec<String> {
    let Ok(output) = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files", "--cached", "--others", "--exclude-standard"])
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::to_owned)
        .collect()
}

/// Puts the picker where every component can find it.
pub fn provide(picker: Picker) {
    zgui::reactive::provide_local_context(picker);
}

/// The picker, from inside a component.
///
/// # Panics
///
/// If none was provided above this component, which is a wiring mistake.
#[must_use]
pub fn use_picker() -> Picker {
    zgui::reactive::use_local_context::<Picker>().expect("a picker is provided at the root")
}
