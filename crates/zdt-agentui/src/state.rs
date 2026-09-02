//! What the surface knows.

use std::rc::Rc;

use zdt_agent::ask::{Ask, AskKind, Decision};
use zdt_agent::thread::{ThreadId, ThreadShell};
use zdt_agent_client::AgentClient;
use zgui::reactive::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};

use crate::host::Host;

/// Which of the two faces the window shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Screen {
    /// The editor: the tree, the splits, the buffer line.
    Editor,
    /// The chat view of the selected thread.
    Agent,
}

/// Where inside the surface the keyboard belongs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Want {
    /// The sidebar's list.
    List,
    /// The timeline, scrolled and answered with normal-mode keys.
    Chat,
    /// The composer.
    Composer,
    /// The diff review surface.
    Review,
    /// The commit modal.
    Commit,
    /// The workflow modal.
    Workflow,
    /// The sidebar's search field.
    Filter,
}

/// A span of changes on review: one turn's, or the whole thread's.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Review {
    /// The directory the changes live in.
    pub root: std::path::PathBuf,
    /// What the surface's header says.
    pub title: String,
    /// The checkpoint the span starts at.
    pub before: String,
    /// The checkpoint it ends at.
    pub after: String,
    /// The turn a revert would undo, when the span is one turn.
    pub turn: Option<i64>,
}

/// The commit modal's standing state: which directory, and whether the commit pushes.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Committing {
    /// The directory whose changes are committed.
    ///
    /// A directory and never a thread: what a person commits is the work in front of them, which
    /// a thread may have written, or a person, or both. A session with no thread at all still has
    /// changes to commit.
    pub root: std::path::PathBuf,
    /// The thread working there, when one is. Only a worktree thread's own branch reads it.
    pub thread: Option<ThreadId>,
    /// Whether the commit is pushed afterwards.
    pub push: bool,
}

/// Which shelf of the sidebar a row sits on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shelf {
    /// Held at the top by hand.
    Pinned,
    /// The working set: everything not put away.
    Active,
    /// Asleep until a moment.
    Snoozed,
    /// Put away as done.
    Settled,
    /// Archived, shown only when asked for.
    Archived,
}

impl Shelf {
    /// What the shelf's header says.
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::Pinned => "Pinned",
            Self::Active => "Active",
            Self::Snoozed => "Snoozed",
            Self::Settled => "Settled",
            Self::Archived => "Archived",
        }
    }

    /// The shelf `shell` sits on, measured against `now`.
    #[must_use]
    pub fn of(shell: &ThreadShell, now: u64) -> Self {
        if shell.archived {
            Self::Archived
        } else if shell.pinned > 0.0 {
            Self::Pinned
        } else if shell.snoozed_until > now {
            Self::Snoozed
        } else if shell.settled {
            Self::Settled
        } else {
            Self::Active
        }
    }
}

/// One row of the sidebar: a shelf's header, or a thread on it.
#[derive(Clone, PartialEq, Debug)]
pub enum SideRow {
    /// A shelf begins.
    Header(Shelf),
    /// One thread. Boxed to keep the rows list small.
    Thread(Box<ThreadShell>),
}

impl SideRow {
    /// A key that tells every row apart: shelves below zero, threads by their id.
    #[must_use]
    pub fn key(&self) -> i64 {
        match self {
            Self::Header(Shelf::Pinned) => -1,
            Self::Header(Shelf::Active) => -2,
            Self::Header(Shelf::Snoozed) => -3,
            Self::Header(Shelf::Settled) => -4,
            Self::Header(Shelf::Archived) => -5,
            Self::Thread(shell) => shell.id.0,
        }
    }
}

/// Which of the composer's menus is open.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MenuKind {
    /// The permission mode.
    Mode,
    /// The model.
    Model,
    /// The reasoning effort.
    Effort,
}

/// One row of an open menu.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MenuRow {
    /// What the row says.
    pub label: String,
    /// One line under it, when there is one.
    pub description: String,
    /// Whether it is what the thread runs as now.
    pub current: bool,
}

/// The surface's state.
///
/// Cloning one is cloning a handle: the sidebar, the chat view and the actions all drive the
/// same signals.
#[derive(Clone)]
pub struct AgentUi {
    inner: Rc<Inner>,
}

struct Inner {
    client: AgentClient,
    host: Rc<dyn Host>,
    /// Whether the sidebar is on screen.
    open: RwSignal<bool, LocalStorage>,
    /// Which face the window shows.
    screen: RwSignal<Screen, LocalStorage>,
    /// Which thread the chat view shows.
    selected: RwSignal<Option<ThreadId>, LocalStorage>,
    /// The directory of the session the editor shows.
    ///
    /// What the selection is measured against: a thread belongs to a directory, and the surface
    /// must never speak for a directory the person is not looking at.
    here: RwSignal<Option<std::path::PathBuf>, LocalStorage>,
    /// The directory a thread has been asked for and not yet arrived in.
    ///
    /// One ask per directory: the rows and the answer race, and a second ask made while the first
    /// is in flight is a second thread nobody wanted.
    creating: std::cell::RefCell<Option<std::path::PathBuf>>,
    /// Where the sidebar's caret is.
    at: RwSignal<usize, LocalStorage>,
    /// Where inside the surface the keyboard belongs.
    wants: RwSignal<Want, LocalStorage>,
    /// The options taken so far in a question ask, one list per question already answered.
    picked: RwSignal<Vec<Vec<String>>, LocalStorage>,
    /// The options toggled in the question being answered, when it takes several.
    toggled: RwSignal<Vec<String>, LocalStorage>,
    /// The open composer menu, and where its caret is.
    menu: RwSignal<Option<(MenuKind, usize)>, LocalStorage>,
    /// The span of changes on review, while one is.
    review: RwSignal<Option<Review>, LocalStorage>,
    /// The review's diffs, loaded off the span's checkpoints. Shared so a read costs nothing.
    review_files: RwSignal<Rc<Vec<zdt_git::FileDiff>>, LocalStorage>,
    /// The syntax colours of each reviewed file, by its path.
    review_marks:
        RwSignal<Rc<std::collections::HashMap<String, zdt_syntax::DiffMarks>>, LocalStorage>,
    /// Which file section the review's caret is on.
    review_at: RwSignal<usize, LocalStorage>,
    /// Whether the review lays old and new side by side.
    review_split: RwSignal<bool, LocalStorage>,
    /// Whether whitespace-only hunks are hidden.
    review_ws: RwSignal<bool, LocalStorage>,
    /// Whether the archived shelf is on screen.
    archived_shown: RwSignal<bool, LocalStorage>,
    /// The commit modal's state, while it is open.
    committing: RwSignal<Option<Committing>, LocalStorage>,
    /// Which runner the workflow modal shows, while it is open.
    workflow_open: RwSignal<Option<String>, LocalStorage>,
    /// What the sidebar's search holds. Empty shows everything.
    filter: RwSignal<String, LocalStorage>,
    /// The search field's element, once the sidebar has built it.
    search_field: RwSignal<Option<zgui::view::NodeRef>, LocalStorage>,
    /// How wide the sidebar is drawn, live while its edge is dragged.
    side_width: zdt_view::PanelWidth,
}

/// How narrow and how wide the sidebar may be, matching what `agent.css` will honour.
pub const SIDE_NARROWEST: u32 = 200;
pub const SIDE_WIDEST: u32 = 480;

impl AgentUi {
    /// A closed surface over `client`, inside `host`.
    #[must_use]
    pub fn new(client: AgentClient, host: Rc<dyn Host>) -> Self {
        Self {
            inner: Rc::new(Inner {
                client,
                host,
                open: RwSignal::new_local(false),
                screen: RwSignal::new_local(Screen::Editor),
                selected: RwSignal::new_local(None),
                here: RwSignal::new_local(None),
                creating: std::cell::RefCell::new(None),
                at: RwSignal::new_local(0),
                wants: RwSignal::new_local(Want::List),
                picked: RwSignal::new_local(Vec::new()),
                toggled: RwSignal::new_local(Vec::new()),
                menu: RwSignal::new_local(None),
                review: RwSignal::new_local(None),
                review_files: RwSignal::new_local(Rc::new(Vec::new())),
                review_marks: RwSignal::new_local(Rc::new(std::collections::HashMap::new())),
                review_at: RwSignal::new_local(0),
                review_split: RwSignal::new_local(false),
                review_ws: RwSignal::new_local(false),
                archived_shown: RwSignal::new_local(false),
                committing: RwSignal::new_local(None),
                workflow_open: RwSignal::new_local(None),
                filter: RwSignal::new_local(String::new()),
                search_field: RwSignal::new_local(None),
                side_width: zdt_view::PanelWidth::new(280, SIDE_NARROWEST, SIDE_WIDEST),
            }),
        }
    }

    /// How wide the sidebar is drawn.
    #[must_use]
    pub fn side_width(&self) -> &zdt_view::PanelWidth {
        &self.inner.side_width
    }

    /// The connection behind the surface.
    #[must_use]
    pub fn client(&self) -> &AgentClient {
        &self.inner.client
    }

    /// The application around the surface.
    #[must_use]
    pub fn host(&self) -> &Rc<dyn Host> {
        &self.inner.host
    }

    // ---- The sidebar -------------------------------------------------------------------------

    /// Whether the sidebar is on screen. Tracked.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.inner.open.get()
    }

    /// Shows or hides the sidebar, moving nothing else.
    pub fn set_open(&self, open: bool) {
        if self.inner.open.get_untracked() != open {
            self.inner.open.set(open);
        }
    }

    /// Turns the sidebar over, and moves the keyboard with it.
    pub fn toggle_sidebar(&self) {
        let opening = !self.inner.open.get_untracked();
        self.inner.open.set(opening);
        if opening {
            self.caret_to_selection();
            self.inner.wants.set(Want::List);
            self.inner.host.focus_agent();
        } else {
            if self.inner.screen.get_untracked() == Screen::Agent {
                self.inner.screen.set(Screen::Editor);
            }
            self.inner.host.leave();
        }
    }

    /// Where the sidebar's caret is, as an index into [`Self::visible`]. Tracked.
    #[must_use]
    pub fn at(&self) -> usize {
        self.inner.at.get()
    }

    /// The sidebar's rows: shelf headers and the threads on them. Tracked.
    ///
    /// The active shelf carries no header; everything parked is labeled. The archived shelf is
    /// only there while it is asked for.
    #[must_use]
    pub fn rows(&self) -> Vec<SideRow> {
        Self::rows_of(
            &self.inner.client.threads(),
            self.inner.archived_shown.get(),
            &self.inner.filter.get(),
        )
    }

    /// The threads on screen, in the order the rows show them. Tracked.
    #[must_use]
    pub fn visible(&self) -> Vec<ThreadShell> {
        Self::threads_of(self.rows())
    }

    /// The ids of the threads on screen, in row order, notifying only when the order moves.
    ///
    /// Made once by the sidebar in its own scope and read by every row: a row that derived the
    /// order for itself would sort the whole list once per row on every change to any thread.
    #[must_use]
    pub fn visible_order(&self) -> RwSignal<Vec<ThreadId>, LocalStorage> {
        let agent = self.clone();
        zdt_view::settled(move || agent.visible().into_iter().map(|shell| shell.id).collect())
    }

    /// The same, without subscribing.
    #[must_use]
    fn visible_untracked(&self) -> Vec<ThreadShell> {
        Self::threads_of(Self::rows_of(
            &self.inner.client.threads_untracked(),
            self.inner.archived_shown.get_untracked(),
            &self.inner.filter.get_untracked(),
        ))
    }

    /// The rows for `threads`, shelved and ordered.
    fn rows_of(threads: &[ThreadShell], archived_shown: bool, filter: &str) -> Vec<SideRow> {
        // The search sees everything, shelves included: a filtered list is already an answer,
        // and hiding archived matches from it would make the search lie.
        let threads: Vec<ThreadShell> = if filter.trim().is_empty() {
            threads.to_vec()
        } else {
            let lines: Vec<String> = threads
                .iter()
                .map(|shell| format!("{} {} {}", shell.title, shell.project, shell.branch))
                .collect();
            let kept: std::collections::HashSet<usize> =
                zdt_core::search::fuzzy::rank(&lines, filter.trim(), lines.len())
                    .into_iter()
                    .map(|found| found.index)
                    .collect();
            threads
                .iter()
                .enumerate()
                .filter(|(index, _)| kept.contains(index))
                .map(|(_, shell)| shell.clone())
                .collect()
        };
        let searching = !filter.trim().is_empty();
        let archived_shown = archived_shown || searching;
        let threads = &threads[..];
        let now = zdt_core::state::now_ms();
        let mut pinned = Vec::new();
        let mut active = Vec::new();
        let mut snoozed = Vec::new();
        let mut settled = Vec::new();
        let mut archived = Vec::new();
        for shell in threads {
            match Shelf::of(shell, now) {
                Shelf::Pinned => pinned.push(shell.clone()),
                Shelf::Active => active.push(shell.clone()),
                Shelf::Snoozed => snoozed.push(shell.clone()),
                Shelf::Settled => settled.push(shell.clone()),
                Shelf::Archived => archived.push(shell.clone()),
            }
        }
        pinned.sort_by(|left, right| right.pinned.total_cmp(&left.pinned));
        snoozed.sort_by_key(|shell| shell.snoozed_until);

        let mut rows = Vec::new();
        if !pinned.is_empty() {
            rows.push(SideRow::Header(Shelf::Pinned));
            rows.extend(
                pinned
                    .into_iter()
                    .map(|shell| SideRow::Thread(Box::new(shell))),
            );
        }
        rows.extend(
            active
                .into_iter()
                .map(|shell| SideRow::Thread(Box::new(shell))),
        );
        if !snoozed.is_empty() {
            rows.push(SideRow::Header(Shelf::Snoozed));
            rows.extend(
                snoozed
                    .into_iter()
                    .map(|shell| SideRow::Thread(Box::new(shell))),
            );
        }
        if !settled.is_empty() {
            rows.push(SideRow::Header(Shelf::Settled));
            rows.extend(
                settled
                    .into_iter()
                    .map(|shell| SideRow::Thread(Box::new(shell))),
            );
        }
        if archived_shown && !archived.is_empty() {
            rows.push(SideRow::Header(Shelf::Archived));
            rows.extend(
                archived
                    .into_iter()
                    .map(|shell| SideRow::Thread(Box::new(shell))),
            );
        }
        rows
    }

    /// Only the threads of `rows`, in order.
    fn threads_of(rows: Vec<SideRow>) -> Vec<ThreadShell> {
        rows.into_iter()
            .filter_map(|row| match row {
                SideRow::Thread(shell) => Some(*shell),
                SideRow::Header(_) => None,
            })
            .collect()
    }

    /// Whether the archived shelf is on screen. Tracked.
    #[must_use]
    pub fn archived_shown(&self) -> bool {
        self.inner.archived_shown.get()
    }

    /// Turns the archived shelf over.
    pub fn toggle_archived(&self) {
        self.inner.archived_shown.update(|held| *held = !*held);
    }

    /// What the sidebar's search holds. Tracked.
    #[must_use]
    pub fn filter(&self) -> String {
        self.inner.filter.get()
    }

    /// Filters the sidebar by `typed`, and puts the caret back on the first row.
    pub fn set_filter(&self, typed: String) {
        if self.inner.filter.with_untracked(|held| *held != typed) {
            self.inner.filter.set(typed);
            self.inner.at.set(0);
        }
    }

    /// Where the search field's element is written down, when the sidebar builds it.
    pub fn register_search(&self, field: zgui::view::NodeRef) {
        self.inner.search_field.set(Some(field));
    }

    /// The search field's element, once the sidebar has built it.
    #[must_use]
    pub fn search_node(&self) -> Option<zgui::view::NodeRef> {
        self.inner.search_field.get_untracked()
    }

    /// Says the keyboard is in the search, which its own focus does.
    pub fn to_filter(&self) {
        if self.inner.wants.get_untracked() != Want::Filter {
            self.inner.wants.set(Want::Filter);
        }
    }

    /// Opens the sidebar and puts the keyboard in its search.
    pub fn focus_filter(&self) {
        self.inner.open.set(true);
        self.inner.wants.set(Want::Filter);
        self.inner.host.focus_agent();
        // Again a frame later, the way every surface-opening press does it.
        if let Some(timers) = zgui::view::time::Timers::current() {
            let host = std::rc::Rc::clone(&self.inner.host);
            let handle = timers.set_timeout(std::time::Duration::ZERO, move || host.focus_agent());
            std::mem::forget(handle);
        }
    }

    /// Moves the caret by `delta`, staying on the list.
    pub fn step(&self, delta: isize) {
        let count = self.visible_untracked().len();
        if count == 0 {
            return;
        }
        let at = self.inner.at.get_untracked() as isize;
        let moved = (at + delta).clamp(0, count as isize - 1) as usize;
        if moved != at as usize {
            self.inner.at.set(moved);
        }
    }

    /// Puts the caret on the first row.
    pub fn to_top(&self) {
        self.inner.at.set(0);
    }

    /// Puts the caret on the last row.
    pub fn to_bottom(&self) {
        let count = self.visible_untracked().len();
        self.inner.at.set(count.saturating_sub(1));
    }

    /// Puts the caret on `index`.
    pub fn go_to(&self, index: usize) {
        self.inner.at.set(index);
    }

    // ---- The screen and the selection --------------------------------------------------------

    /// Which face the window shows. Tracked.
    #[must_use]
    pub fn screen(&self) -> Screen {
        self.inner.screen.get()
    }

    /// Turns the window between the editor and the chat.
    ///
    /// The chat always shows the work of the session on screen: turning to it takes that
    /// directory's last thread, and makes one when the directory has none.
    pub fn toggle_screen(&self) {
        match self.inner.screen.get_untracked() {
            Screen::Agent => {
                self.inner.screen.set(Screen::Editor);
                self.inner.host.leave();
            }
            Screen::Editor => {
                self.inner.open.set(true);
                self.inner.screen.set(Screen::Agent);
                // A deliberate turn to the chat asks again: an ask that went nowhere must not
                // leave the directory without a thread for good.
                self.inner.creating.borrow_mut().take();
                // After the screen, because what a directory with no thread means depends on
                // which face is being shown.
                self.settle_selection();
                self.caret_to_selection();
                self.inner.wants.set(Want::List);
                self.inner.host.focus_agent();
            }
        }
    }

    // ---- The session on screen ---------------------------------------------------------------

    /// Says which session the editor shows now.
    ///
    /// Called whenever a window puts a session on screen. What follows from it is [`Self::settle`],
    /// which every arrival of the daemon's rows runs again.
    pub fn showing_project(&self, root: &std::path::Path) {
        if self
            .inner
            .here
            .with_untracked(|held| held.as_deref() == Some(root))
        {
            return;
        }
        self.inner.here.set(Some(root.to_path_buf()));
        // A thread asked for in the directory somebody has just left is no longer wanted here.
        self.inner.creating.borrow_mut().take();
    }

    /// The directory of the session on screen. Tracked.
    #[must_use]
    pub fn here(&self) -> Option<std::path::PathBuf> {
        self.inner.here.get()
    }

    /// The same, falling back to what the host says while no window has spoken yet.
    #[must_use]
    fn here_untracked(&self) -> Option<std::path::PathBuf> {
        self.inner
            .here
            .get_untracked()
            .or_else(|| self.inner.host.project_root())
    }

    /// The thread in `root` a person worked in last, when it has one.
    #[must_use]
    pub fn thread_in(&self, root: &std::path::Path) -> Option<ThreadShell> {
        last_in(&self.inner.client.threads_untracked(), root)
    }

    /// Puts the selection back in step with the session on screen.
    ///
    /// The rule is [`answer_for`]; this is what carrying each answer out means.
    pub fn settle_selection(&self) {
        let Some(root) = self.here_untracked() else {
            return;
        };
        let answer = answer_for(
            &root,
            self.selected_shell_untracked().as_ref(),
            &self.inner.client.threads_untracked(),
            Asked {
                screen: self.inner.screen.get_untracked(),
                listed: self.inner.client.has_listed_untracked(),
                asked_in: self.inner.creating.borrow().clone(),
            },
        );

        match answer {
            Answer::Keep => {
                self.inner.creating.borrow_mut().take();
            }
            Answer::Show(shell) => {
                self.inner.creating.borrow_mut().take();
                self.adopt(&shell);
            }
            Answer::Make => {
                self.clear_selection();
                *self.inner.creating.borrow_mut() = Some(root.clone());
                self.inner.client.create(root, String::new());
            }
            Answer::Nothing => self.clear_selection(),
        }
    }

    /// Which thread the chat view shows. Tracked.
    #[must_use]
    pub fn selected(&self) -> Option<ThreadId> {
        self.inner.selected.get()
    }

    /// The same, without subscribing.
    #[must_use]
    pub fn selected_untracked(&self) -> Option<ThreadId> {
        self.inner.selected.get_untracked()
    }

    /// The selected thread's shell, as the daemon last said. Tracked.
    #[must_use]
    pub fn selected_shell(&self) -> Option<ThreadShell> {
        self.selected().and_then(|id| self.inner.client.thread(id))
    }

    /// The row under `index` of the visible list, as the daemon last said.
    #[must_use]
    pub fn shell_at(&self, index: usize) -> Option<ThreadShell> {
        self.visible_untracked().into_iter().nth(index)
    }

    /// Opens `thread` wherever it is shelved, unfolding the archived shelf when it must.
    pub fn open_thread(&self, thread: ThreadId) {
        let mut visible = self.visible_untracked();
        if !visible.iter().any(|shell| shell.id == thread) {
            self.inner.archived_shown.set(true);
            visible = self.visible_untracked();
        }
        if let Some(index) = visible.iter().position(|shell| shell.id == thread) {
            self.open_at(index);
        }
    }

    /// The selected thread's provider mark, for the model chip and its menu. Tracked.
    #[must_use]
    pub fn provider_mark(&self) -> Option<&'static str> {
        self.selected_shell()
            .and_then(|shell| zdt_icons::brand(&shell.provider))
    }

    /// Opens the row under the caret: selects it and follows it.
    ///
    /// The screen stays whichever it was: choosing a thread in editor view switches the session
    /// underneath and leaves the editor on screen, and choosing one in the chat view stays
    /// there. Which face to look at is the toggle's decision alone.
    pub fn open_at(&self, index: usize) {
        let Some(shell) = self.shell_at(index) else {
            return;
        };
        self.inner.at.set(index);
        self.select(&shell);
        self.inner.wants.set(Want::List);
        self.inner.host.focus_agent();
    }

    /// Makes `shell` the one the chat shows, and the editor follow its directory.
    pub fn select(&self, shell: &ThreadShell) {
        self.adopt(shell);
        // A worktree thread's fresh session starts from its project's saved editor state.
        let inherits = (shell.worktree && !shell.project_root.as_os_str().is_empty())
            .then_some(shell.project_root.as_path());
        self.inner.host.open_project(&shell.root, inherits);
    }

    /// The same, leaving the editor where it is.
    ///
    /// What following the editor uses: the session is already the thread's own, and opening it
    /// again would take the keyboard back into the surface somebody has just left.
    fn adopt(&self, shell: &ThreadShell) {
        if self.inner.selected.get_untracked() != Some(shell.id) {
            self.inner.selected.set(Some(shell.id));
            self.clear_answers();
            self.close_review();
            self.close_commit();
        }
        self.inner.client.watch(shell.id);
    }

    /// Leaves the chat showing nothing.
    fn clear_selection(&self) {
        if self.inner.selected.get_untracked().is_none() {
            return;
        }
        self.inner.selected.set(None);
        self.clear_answers();
        self.close_review();
        self.close_commit();
    }

    /// Forgets a selection that named `thread`, which deleting it does.
    pub fn deselect(&self, thread: ThreadId) {
        if self.inner.selected.get_untracked() == Some(thread) {
            self.inner.selected.set(None);
        }
    }

    // ---- The keyboard inside the surface -----------------------------------------------------

    /// Where inside the surface the keyboard belongs. Tracked.
    #[must_use]
    pub fn wants(&self) -> Want {
        self.inner.wants.get()
    }

    /// Whether the list's caret is on screen: the list has the keyboard for motions. Tracked.
    #[must_use]
    pub fn caret_shown(&self) -> bool {
        self.inner.wants.get() == Want::List && self.inner.host.has_keyboard()
    }

    /// Sends the keyboard to the composer.
    pub fn compose(&self) {
        self.inner.screen.set(Screen::Agent);
        // A composer with no thread under it has nowhere to send what is typed.
        self.settle_selection();
        self.inner.wants.set(Want::Composer);
        self.inner.host.focus_agent();
    }

    /// Adopts a focus that landed on the composer.
    ///
    /// A reaction, never a command: the screen is left as it stands. A focus passing through
    /// while the window turns to the editor is the turn's side effect, and following it would
    /// turn the window straight back.
    pub fn composer_focused(&self) {
        if self.inner.screen.get_untracked() != Screen::Agent {
            return;
        }
        if self.inner.wants.get_untracked() != Want::Composer {
            self.inner.wants.set(Want::Composer);
        }
        self.inner.host.took_keyboard();
    }

    /// Adopts a focus that landed on the timeline. See [`Self::composer_focused`].
    pub fn chat_focused(&self) {
        if self.inner.screen.get_untracked() != Screen::Agent {
            return;
        }
        if self.inner.wants.get_untracked() != Want::Chat {
            self.inner.wants.set(Want::Chat);
        }
        self.inner.host.took_keyboard();
    }

    /// Sends the keyboard to the timeline.
    pub fn to_chat(&self) {
        self.inner.screen.set(Screen::Agent);
        self.settle_selection();
        if self.inner.wants.get_untracked() != Want::Chat {
            self.inner.wants.set(Want::Chat);
        }
        self.inner.host.focus_agent();
    }

    /// Sends the keyboard back to the sidebar's list.
    ///
    /// The caret lands on the selected thread: motions start from what is on screen, wherever
    /// the caret stood last time.
    pub fn to_list(&self) {
        self.caret_to_selection();
        if self.inner.wants.get_untracked() != Want::List {
            self.inner.wants.set(Want::List);
        }
    }

    /// Puts the caret on the selected thread, when it is anywhere on the list.
    fn caret_to_selection(&self) {
        let Some(selected) = self.inner.selected.get_untracked() else {
            return;
        };
        if let Some(index) = self
            .visible_untracked()
            .iter()
            .position(|shell| shell.id == selected)
        {
            self.inner.at.set(index);
        }
    }

    // ---- Talking to the daemon ---------------------------------------------------------------

    /// Sends `text` into the selected thread.
    pub fn send(&self, text: String) {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return;
        }
        let Some(thread) = self.selected_untracked() else {
            self.inner.host.say("no thread is selected");
            return;
        };
        self.inner.client.send(thread, trimmed.to_owned());
    }

    /// Stops the selected thread's turn.
    pub fn interrupt(&self) {
        match self.selected_untracked() {
            Some(thread) => self.inner.client.interrupt(thread),
            None => self.inner.host.say("no thread is selected"),
        }
    }

    /// Makes a thread that works in `root`, on the named provider instance. Empty means the
    /// daemon's default. The selection follows once the daemon answers.
    pub fn create_in(&self, root: std::path::PathBuf, instance: String) {
        self.inner.client.create(root, instance);
    }

    /// Takes the row under the caret away, history included.
    pub fn delete_at(&self) {
        let at = self.inner.at.get_untracked();
        let Some(shell) = self.shell_at(at) else {
            return;
        };
        if shell.is_working() {
            self.inner
                .host
                .say("the thread is working; interrupt it first");
            return;
        }
        self.deselect(shell.id);
        self.inner.client.delete(shell.id);
    }

    // ---- Lifecycle ---------------------------------------------------------------------------

    /// The row under the caret, without subscribing.
    #[must_use]
    pub fn caret_shell(&self) -> Option<ThreadShell> {
        self.shell_at(self.inner.at.get_untracked())
    }

    /// Pins the row under the caret to the top, or unpins it.
    pub fn pin_toggle(&self) {
        let Some(shell) = self.caret_shell() else {
            return;
        };
        if shell.pinned > 0.0 {
            self.inner.client.pin(shell.id, 0.0);
        } else {
            let top = self
                .inner
                .client
                .threads_untracked()
                .iter()
                .map(|held| held.pinned)
                .fold(0.0_f64, f64::max);
            self.inner.client.pin(shell.id, top + 1.0);
        }
    }

    /// Moves the pinned row under the caret up or down among the pinned, by swapping places.
    pub fn pin_move(&self, delta: isize) {
        let Some(shell) = self.caret_shell() else {
            return;
        };
        if shell.pinned <= 0.0 {
            self.inner.host.say("pin the thread first");
            return;
        }
        let mut pinned: Vec<ThreadShell> = self
            .inner
            .client
            .threads_untracked()
            .into_iter()
            .filter(|held| held.pinned > 0.0 && !held.archived)
            .collect();
        pinned.sort_by(|left, right| right.pinned.total_cmp(&left.pinned));
        let Some(here) = pinned.iter().position(|held| held.id == shell.id) else {
            return;
        };
        let there = here as isize + delta;
        if there < 0 || there as usize >= pinned.len() {
            return;
        }
        let other = &pinned[there as usize];
        self.inner.client.pin(shell.id, other.pinned);
        self.inner.client.pin(other.id, shell.pinned);
        // The caret follows the row to where it lands.
        self.inner.at.update(|held| {
            *held = (*held as isize + delta).max(0) as usize;
        });
    }

    /// Settles the row under the caret, or takes it back out.
    pub fn settle_toggle(&self) {
        if let Some(shell) = self.caret_shell() {
            self.inner.client.settle(shell.id, !shell.settled);
        }
    }

    /// Archives the row under the caret, or brings it back.
    pub fn archive_toggle(&self) {
        if let Some(shell) = self.caret_shell() {
            self.inner.client.archive(shell.id, !shell.archived);
        }
    }

    /// Marks the row under the caret unread, or read.
    pub fn unread_toggle(&self) {
        if let Some(shell) = self.caret_shell() {
            self.inner.client.mark_unread(shell.id, !shell.unread);
        }
    }

    /// Puts the row under the caret to sleep until `until_ms`. Zero wakes it.
    pub fn snooze_until(&self, until_ms: u64) {
        if let Some(shell) = self.caret_shell() {
            self.inner.client.snooze(shell.id, until_ms);
        }
    }

    /// Asks for a new name for the row under the caret. An empty answer has one made up.
    pub fn rename_prompt(&self) {
        let Some(shell) = self.caret_shell() else {
            return;
        };
        let client = self.inner.client.clone();
        self.inner.host.ask_line(
            "Rename thread (empty makes a name up)",
            &shell.title,
            std::rc::Rc::new(move |typed: String| {
                client.rename(shell.id, typed.trim().to_owned());
            }),
        );
    }

    /// Offers the provider-side conversations of `instance` and imports the chosen one.
    ///
    /// The daemon scans the provider's own session store; the answer opens a picker, and the
    /// choice becomes a thread with its history read in and its resume cursor kept.
    pub fn import_from(&self, instance: String, provider: String) {
        // The picker is taken now, while the pressing context still answers; the effect below
        // runs later, from nowhere in particular.
        let Some(offer) = self.inner.host.offer() else {
            self.inner
                .host
                .say("there is no picker to offer the conversations in");
            return;
        };
        self.inner.client.list_imports(instance.clone());
        let title: &'static str = match provider.as_str() {
            "claude" => "Import from Claude Code",
            "codex" => "Import from Codex",
            _ => "Import a conversation",
        };
        let surface = self.clone();
        let landing = zgui::reactive::RenderEffect::new(move |done: Option<bool>| {
            if done == Some(true) {
                return true;
            }
            if !surface.inner.client.has_imports() {
                return false;
            }
            let Some((answered, rows)) = surface.inner.client.take_imports() else {
                return true;
            };
            if answered != instance {
                return true;
            }
            if rows.is_empty() {
                surface.inner.host.say("nothing to import there");
                return true;
            }
            let offered: Vec<(String, String)> = rows
                .iter()
                .map(|row| {
                    let place = row
                        .root
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_else(|| row.root.to_string_lossy().into_owned());
                    let age = age_words(row.at_ms);
                    (row.title.clone(), format!("{place} \u{00b7} {age}"))
                })
                .collect();
            let client = surface.inner.client.clone();
            let picking = instance.clone();
            let ids: Vec<String> = rows.into_iter().map(|row| row.id).collect();
            offer(
                title,
                offered,
                std::rc::Rc::new(move |index: usize| {
                    if let Some(id) = ids.get(index) {
                        client.import(picking.clone(), id.clone());
                    }
                }),
            );
            true
        });
        std::mem::forget(landing);
    }

    /// Keeps `text` as `thread`'s unsent draft, when it differs from what is kept.
    pub fn save_draft(&self, thread: ThreadId, text: String) {
        let same = self
            .inner
            .client
            .threads_untracked()
            .iter()
            .find(|held| held.id == thread)
            .is_some_and(|held| held.draft == text);
        if !same {
            self.inner.client.set_draft(thread, text);
        }
    }

    // ---- Asks --------------------------------------------------------------------------------

    /// The ask being decided: the oldest open one of the selected thread. Tracked.
    #[must_use]
    pub fn asking(&self) -> Option<Ask> {
        self.selected()?;
        self.inner.client.asks().into_iter().next()
    }

    /// Which question of a question ask is being answered. Tracked.
    #[must_use]
    pub fn question_at(&self) -> usize {
        self.inner.picked.with(Vec::len)
    }

    /// The options toggled in the question being answered. Tracked.
    #[must_use]
    pub fn toggled(&self) -> Vec<String> {
        self.inner.toggled.get()
    }

    /// Decides the open tool ask.
    pub fn decide(&self, decision: Decision) {
        let Some(thread) = self.selected_untracked() else {
            return;
        };
        let Some(ask) = self.asking_untracked() else {
            return;
        };
        if !matches!(ask.kind, AskKind::Tool { .. }) {
            return;
        }
        self.inner.client.decide(thread, ask.id, decision);
        self.clear_answers();
    }

    /// Takes option `index` of the question being answered.
    ///
    /// A single-choice question moves on; the last answer sends them all. A several-choice
    /// question toggles the option and waits for [`Self::confirm_question`].
    pub fn choose(&self, index: usize) {
        let Some(ask) = self.asking_untracked() else {
            return;
        };
        let AskKind::Question { questions } = &ask.kind else {
            return;
        };
        let at = self.inner.picked.with_untracked(Vec::len);
        let Some(question) = questions.get(at) else {
            return;
        };
        let Some(option) = question.options.get(index) else {
            return;
        };
        if question.multi {
            let label = option.label.clone();
            self.inner.toggled.update(|held| {
                if let Some(at) = held.iter().position(|held| *held == label) {
                    held.remove(at);
                } else {
                    held.push(label);
                }
            });
            return;
        }
        self.push_answer(&ask, questions.len(), vec![option.label.clone()]);
    }

    /// Finishes the several-choice question being answered.
    pub fn confirm_question(&self) {
        let Some(ask) = self.asking_untracked() else {
            return;
        };
        let AskKind::Question { questions } = &ask.kind else {
            return;
        };
        let taken = self.inner.toggled.get_untracked();
        if taken.is_empty() {
            self.inner.host.say("choose at least one option first");
            return;
        }
        self.inner.toggled.set(Vec::new());
        self.push_answer(&ask, questions.len(), taken);
    }

    /// Adds one question's answer, sending the lot when it was the last.
    fn push_answer(&self, ask: &Ask, questions: usize, answer: Vec<String>) {
        let mut picked = self.inner.picked.get_untracked();
        picked.push(answer);
        if picked.len() >= questions {
            if let Some(thread) = self.selected_untracked() {
                self.inner.client.answer(thread, ask.id.clone(), picked);
            }
            self.clear_answers();
        } else {
            self.inner.picked.set(picked);
        }
    }

    /// Forgets half-taken answers, which a new ask or a new selection does.
    pub fn clear_answers(&self) {
        if self.inner.picked.with_untracked(|held| !held.is_empty()) {
            self.inner.picked.set(Vec::new());
        }
        if self.inner.toggled.with_untracked(|held| !held.is_empty()) {
            self.inner.toggled.set(Vec::new());
        }
    }

    /// The ask being decided, without subscribing.
    #[must_use]
    fn asking_untracked(&self) -> Option<Ask> {
        self.inner.selected.get_untracked()?;
        self.inner.client.asks_untracked().into_iter().next()
    }

    // ---- The composer's menus ----------------------------------------------------------------

    /// The open menu and its caret. Tracked.
    #[must_use]
    pub fn menu(&self) -> Option<(MenuKind, usize)> {
        self.inner.menu.get()
    }

    /// Whether a menu is open, without subscribing.
    #[must_use]
    pub fn menu_open(&self) -> bool {
        self.inner.menu.get_untracked().is_some()
    }

    /// Opens `kind`'s menu with the caret on what the thread runs as now.
    pub fn open_menu(&self, kind: MenuKind) {
        if self.selected_untracked().is_none() {
            self.inner.host.say("no thread is selected");
            return;
        }
        let at = self
            .menu_rows_in(kind, false)
            .iter()
            .position(|row| row.current)
            .unwrap_or(0);
        self.inner.menu.set(Some((kind, at)));
    }

    /// Closes whatever menu is open. `true` when one was.
    pub fn close_menu(&self) -> bool {
        if self.inner.menu.get_untracked().is_some() {
            self.inner.menu.set(None);
            return true;
        }
        false
    }

    /// Moves the open menu's caret by `delta`.
    pub fn menu_step(&self, delta: isize) {
        let Some((kind, at)) = self.inner.menu.get_untracked() else {
            return;
        };
        let count = self.menu_rows_in(kind, false).len();
        if count == 0 {
            return;
        }
        let moved = (at as isize + delta).rem_euclid(count as isize) as usize;
        self.inner.menu.set(Some((kind, moved)));
    }

    /// Takes the open menu's row under the caret.
    pub fn menu_take(&self) {
        if let Some((kind, at)) = self.inner.menu.get_untracked() {
            self.menu_choose(kind, at);
        }
    }

    /// Takes row `index` of the open menu. `false` when no menu is open.
    pub fn menu_take_at(&self, index: usize) -> bool {
        if let Some((kind, _)) = self.inner.menu.get_untracked() {
            self.menu_choose(kind, index);
            return true;
        }
        false
    }

    /// Takes row `index` of `kind`'s menu and closes it.
    pub fn menu_choose(&self, kind: MenuKind, index: usize) {
        self.inner.menu.set(None);
        let Some(thread) = self.selected_untracked() else {
            return;
        };
        match kind {
            MenuKind::Mode => {
                if let Some(mode) = zdt_agent::mode::RuntimeMode::CHOICES.get(index) {
                    self.inner.client.set_mode(thread, *mode);
                }
            }
            MenuKind::Model => {
                if let Some(id) = self.model_ids(false).get(index) {
                    self.inner.client.set_model(thread, id.clone());
                }
            }
            MenuKind::Effort => {
                if let Some(id) = self.effort_ids(false).get(index) {
                    self.inner.client.set_effort(thread, id.clone());
                }
            }
        }
    }

    /// The open menu's rows, for whoever draws them. Tracked.
    #[must_use]
    pub fn menu_rows(&self, kind: MenuKind) -> Vec<MenuRow> {
        self.menu_rows_in(kind, true)
    }

    fn menu_rows_in(&self, kind: MenuKind, tracked: bool) -> Vec<MenuRow> {
        let shell = if tracked {
            self.selected_shell()
        } else {
            self.selected_shell_untracked()
        };
        match kind {
            MenuKind::Mode => {
                let current = shell.map(|shell| shell.mode).unwrap_or_default();
                zdt_agent::mode::RuntimeMode::CHOICES
                    .into_iter()
                    .map(|mode| MenuRow {
                        label: mode.label().to_owned(),
                        description: mode.blurb().to_owned(),
                        current: mode == current,
                    })
                    .collect()
            }
            MenuKind::Model => {
                let current = shell.map(|shell| shell.model).unwrap_or_default();
                let ids = self.model_ids(tracked);
                let session = if tracked {
                    self.inner.client.catalog().models
                } else {
                    self.inner.client.catalog_untracked().models
                };
                if session.is_empty() {
                    ids.iter()
                        .map(|id| MenuRow {
                            label: if id.is_empty() {
                                "Default".to_owned()
                            } else {
                                id.clone()
                            },
                            description: String::new(),
                            current: *id == current,
                        })
                        .collect()
                } else {
                    session
                        .into_iter()
                        .zip(ids)
                        .map(|(model, id)| MenuRow {
                            label: if model.label.is_empty() {
                                model.id
                            } else {
                                model.label
                            },
                            description: model.description,
                            current: id == current,
                        })
                        .collect()
                }
            }
            MenuKind::Effort => {
                let current = shell.map(|shell| shell.effort).unwrap_or_default();
                let ids = self.effort_ids(tracked);
                let session = if tracked {
                    self.inner.client.catalog().efforts
                } else {
                    self.inner.client.catalog_untracked().efforts
                };
                session
                    .into_iter()
                    .zip(ids)
                    .map(|(effort, id)| MenuRow {
                        label: if effort.label.is_empty() {
                            effort.id
                        } else {
                            effort.label
                        },
                        description: effort.description,
                        current: id == current,
                    })
                    .collect()
            }
        }
    }

    /// The efforts the session offers. Empty until the catalog has said, which is what hides
    /// the chip. Tracked.
    #[must_use]
    pub fn efforts_known(&self) -> bool {
        self.inner
            .client
            .catalog()
            .efforts
            .iter()
            .any(|effort| !effort.id.is_empty())
    }

    /// What each effort row means to the daemon, aligned with the effort menu's rows.
    ///
    /// The provider's own `default` becomes the empty word, like a model's.
    fn effort_ids(&self, tracked: bool) -> Vec<String> {
        let session = if tracked {
            self.inner.client.catalog().efforts
        } else {
            self.inner.client.catalog_untracked().efforts
        };
        session
            .into_iter()
            .map(|effort| {
                if effort.id == "default" {
                    String::new()
                } else {
                    effort.id
                }
            })
            .collect()
    }

    /// What each model row means to the daemon, aligned with the model menu's rows.
    ///
    /// The provider's own `default` becomes the empty word: an empty model is "the provider
    /// decides", and it holds across sessions and resumes.
    fn model_ids(&self, tracked: bool) -> Vec<String> {
        let session = if tracked {
            self.inner.client.catalog().models
        } else {
            self.inner.client.catalog_untracked().models
        };
        if session.is_empty() {
            let mut ids = vec![String::new()];
            ids.extend(self.inner.host.models());
            ids
        } else {
            session
                .into_iter()
                .map(|model| {
                    if model.id == "default" {
                        String::new()
                    } else {
                        model.id
                    }
                })
                .collect()
        }
    }

    /// The selected thread's shell, without subscribing.
    #[must_use]
    fn selected_shell_untracked(&self) -> Option<ThreadShell> {
        let id = self.inner.selected.get_untracked()?;
        self.inner
            .client
            .threads_untracked()
            .into_iter()
            .find(|shell| shell.id == id)
    }

    // ---- The plan ----------------------------------------------------------------------------

    /// Takes the proposed plan and has it carried out.
    pub fn implement(&self) {
        match self.selected_untracked() {
            Some(thread) => self.inner.client.implement(thread),
            None => self.inner.host.say("no thread is selected"),
        }
    }

    // ---- Review, revert, and git -------------------------------------------------------------

    /// The span of changes on review, while one is. Tracked.
    #[must_use]
    pub fn review(&self) -> Option<Review> {
        self.inner.review.get()
    }

    /// Puts `review` on screen, gives it the keyboard, and loads its diffs off a worker.
    pub fn open_review(&self, review: Review) {
        self.inner.screen.set(Screen::Agent);
        self.inner.review_at.set(0);
        self.inner.review_files.set(Rc::new(Vec::new()));
        self.inner
            .review_marks
            .set(Rc::new(std::collections::HashMap::new()));
        let (root, before, after) = (
            review.root.clone(),
            review.before.clone(),
            review.after.clone(),
        );
        self.inner.review.set(Some(review));
        self.inner.wants.set(Want::Review);
        self.inner.host.focus_agent();
        // Again a frame later: a press that opened the review moves the keyboard onto its own
        // control after this handler, and the surface's sink is re-registered on the coming
        // flush. The second call lands after both.
        if let Some(timers) = zgui::view::time::Timers::current() {
            let host = std::rc::Rc::clone(&self.inner.host);
            let handle = timers.set_timeout(std::time::Duration::ZERO, move || host.focus_agent());
            std::mem::forget(handle);
        }

        let surface = self.clone();
        let wanted = (before.clone(), after.clone());
        zdt_view::detached(async move {
            let loaded = zgui::task::blocking(move || {
                let repo = zdt_git::Repo::open(&root).ok()?;
                let files = zdt_git::checkpoint::changes(&repo, &before, &after).ok()?;
                // Coloured on the worker: a parse is too slow for the interface thread, and
                // the shared cache makes a re-opened review free.
                let marks = files
                    .iter()
                    .map(|file| (file.path.clone(), zdt_syntax::marks_of(file)))
                    .collect::<std::collections::HashMap<_, _>>();
                Some((files, marks))
            })
            .await
            .unwrap_or_default();
            // A stale answer is dropped: the review may have moved on while the worker read.
            let current = surface
                .inner
                .review
                .get_untracked()
                .is_some_and(|held| (held.before, held.after) == wanted);
            if current {
                let (files, marks) = loaded;
                surface.inner.review_marks.set(Rc::new(marks));
                surface.inner.review_files.set(Rc::new(files));
            }
        });
    }

    /// The review's diffs, oldest path first. Tracked.
    #[must_use]
    pub fn review_files(&self) -> Rc<Vec<zdt_git::FileDiff>> {
        self.inner.review_files.get()
    }

    /// The syntax colours of each reviewed file, by its path. Tracked.
    #[must_use]
    pub fn review_marks(&self) -> Rc<std::collections::HashMap<String, zdt_syntax::DiffMarks>> {
        self.inner.review_marks.get()
    }

    /// Which file section the review's caret is on. Tracked.
    #[must_use]
    pub fn review_at(&self) -> usize {
        self.inner.review_at.get()
    }

    /// Moves the review's caret by `delta`, staying on the list.
    pub fn review_step(&self, delta: isize) {
        let count = self.inner.review_files.with_untracked(|files| files.len());
        if count == 0 {
            return;
        }
        let at = self.inner.review_at.get_untracked() as isize;
        let moved = (at + delta).clamp(0, count as isize - 1) as usize;
        if moved != at as usize {
            self.inner.review_at.set(moved);
        }
    }

    /// Puts the review's caret on `index`.
    pub fn review_go_to(&self, index: usize) {
        self.inner.review_at.set(index);
    }

    /// Opens the caret's file in the editor, at its first change.
    pub fn review_open_file(&self) {
        let Some(review) = self.inner.review.get_untracked() else {
            return;
        };
        let at = self.inner.review_at.get_untracked();
        self.inner.review_files.with_untracked(|files| {
            let Some(file) = files.get(at) else {
                return;
            };
            let line = file
                .hunks
                .first()
                .map(|hunk| u64::from(hunk.new_start.max(1)));
            self.inner
                .host
                .open_file(&review.root.join(&file.path), line);
        });
    }

    /// Whether the review lays old and new side by side. Tracked.
    #[must_use]
    pub fn review_split(&self) -> bool {
        self.inner.review_split.get()
    }

    /// Turns the side-by-side layout over.
    pub fn toggle_review_split(&self) {
        self.inner.review_split.update(|held| *held = !*held);
    }

    /// Whether whitespace-only hunks are hidden. Tracked.
    #[must_use]
    pub fn review_ws(&self) -> bool {
        self.inner.review_ws.get()
    }

    /// Turns the whitespace filter over.
    pub fn toggle_review_ws(&self) {
        self.inner.review_ws.update(|held| *held = !*held);
    }

    /// Takes the review off screen. `true` when one was there.
    pub fn close_review(&self) -> bool {
        if self.inner.review.get_untracked().is_none() {
            return false;
        }
        self.inner.review.set(None);
        if self.inner.wants.get_untracked() == Want::Review {
            self.inner.wants.set(Want::Chat);
        }
        true
    }

    /// Opens the review of everything the selected thread has changed.
    ///
    /// The span runs from the first turn's checkpoint to the last one captured, read off the
    /// timeline's diff rows.
    pub fn review_thread(&self) {
        let Some(shell) = self.selected_shell_untracked() else {
            self.inner.host.say("no thread is selected");
            return;
        };
        let diffs = self.turn_diffs();
        let (Some(first), Some(last)) = (diffs.first(), diffs.last()) else {
            self.inner
                .host
                .say("the thread has not changed anything yet");
            return;
        };
        self.open_review(Review {
            root: shell.root,
            title: "All changes".to_owned(),
            before: first.before.clone(),
            after: last.after.clone(),
            turn: None,
        });
    }

    /// Opens the review of one turn's changes, from its diff row.
    pub fn review_turn(&self, diff: &zdt_agent::change::TurnDiff) {
        let Some(shell) = self.selected_shell_untracked() else {
            return;
        };
        self.open_review(Review {
            root: shell.root,
            title: "Turn changes".to_owned(),
            before: diff.before.clone(),
            after: diff.after.clone(),
            turn: Some(diff.turn),
        });
    }

    /// Every turn diff in the timeline, oldest first.
    fn turn_diffs(&self) -> Vec<zdt_agent::change::TurnDiff> {
        self.inner
            .client
            .items()
            .into_iter()
            .filter(|item| item.kind == zdt_agent::thread::ItemKind::Diff)
            .filter_map(|item| zdt_agent::change::TurnDiff::decode(&item.detail))
            .collect()
    }

    /// Asks for one word of certainty, then puts the thread back to before `turn` ran.
    pub fn revert_turn(&self, turn: i64) {
        let Some(thread) = self.selected_untracked() else {
            return;
        };
        let surface = self.clone();
        self.inner.host.ask_line(
            "Revert this turn? The working tree and the conversation go back. y or n",
            "",
            std::rc::Rc::new(move |typed: String| {
                if typed.trim().eq_ignore_ascii_case("y") {
                    surface.close_review();
                    surface.inner.client.revert(thread, turn);
                }
            }),
        );
    }

    /// Puts the last turn back, which is what the key without a chosen row means.
    pub fn revert_last(&self) {
        match self.turn_diffs().last() {
            Some(diff) => self.revert_turn(diff.turn),
            None => self.inner.host.say("there is no turn to revert"),
        }
    }

    /// Opens the commit modal for the session on screen. `push` sends the commit on afterwards.
    ///
    /// The directory and never the selection. What is committed is every local change in the
    /// session's own tree, whoever made them: a thread is not wanted for it, and the sidebar may
    /// name one working somewhere else entirely.
    ///
    /// The scan and the draft start at once; the fields fill as the answers land, and nothing
    /// is committed until a person says so.
    pub fn open_commit(&self, push: bool) {
        let Some(root) = self.here_untracked() else {
            self.inner.host.say("there is nothing here to commit");
            return;
        };
        // Carried only so a worktree thread's branch follows a commit onto a new one: the thread
        // on screen when it works here, and the directory's last one otherwise.
        let thread = match self.selected_shell_untracked() {
            Some(shell) if shell.root == root => Some(shell.id),
            _ => self.thread_in(&root).map(|shell| shell.id),
        };
        self.inner.client.draft_commit(root.clone());
        self.inner
            .committing
            .set(Some(Committing { root, thread, push }));
        self.inner.wants.set(Want::Commit);
        self.inner.host.focus_agent();
        // Again a frame later, the same way the review takes the keyboard: the press that
        // opened the modal lands focus after this handler.
        if let Some(timers) = zgui::view::time::Timers::current() {
            let host = std::rc::Rc::clone(&self.inner.host);
            let handle = timers.set_timeout(std::time::Duration::ZERO, move || host.focus_agent());
            std::mem::forget(handle);
        }
    }

    /// Whether there is a session whose changes could be committed. Tracked.
    ///
    /// What the button asks before drawing itself. A thread is never wanted for the answer.
    #[must_use]
    pub fn can_commit(&self) -> bool {
        self.here().is_some() || self.inner.host.project_root().is_some()
    }

    /// The commit modal's state, while it is open. Tracked.
    #[must_use]
    pub fn committing(&self) -> Option<Committing> {
        self.inner.committing.get()
    }

    /// Opens the workflow modal over `runner`, one of the watched thread's workflows.
    pub fn open_workflow(&self, runner: String) {
        self.inner.workflow_open.set(Some(runner));
        self.inner.wants.set(Want::Workflow);
        self.inner.host.focus_agent();
        // Again a frame later; see `open_commit`.
        if let Some(timers) = zgui::view::time::Timers::current() {
            let host = std::rc::Rc::clone(&self.inner.host);
            let handle = timers.set_timeout(std::time::Duration::ZERO, move || host.focus_agent());
            std::mem::forget(handle);
        }
    }

    /// Which runner the workflow modal shows, while it is open. Tracked.
    #[must_use]
    pub fn workflow_open(&self) -> Option<String> {
        self.inner.workflow_open.get()
    }

    /// Takes the workflow modal off screen. `true` when it was there.
    pub fn close_workflow(&self) -> bool {
        if self.inner.workflow_open.get_untracked().is_none() {
            return false;
        }
        self.inner.workflow_open.set(None);
        if self.inner.wants.get_untracked() == Want::Workflow {
            if self.inner.screen.get_untracked() == Screen::Agent {
                self.inner.wants.set(Want::Chat);
            } else {
                self.inner.wants.set(Want::List);
                self.inner.host.leave();
            }
        }
        true
    }

    /// Takes the commit modal off screen. `true` when it was there.
    pub fn close_commit(&self) -> bool {
        if self.inner.committing.get_untracked().is_none() {
            return false;
        }
        self.inner.committing.set(None);
        if self.inner.wants.get_untracked() == Want::Commit {
            if self.inner.screen.get_untracked() == Screen::Agent {
                self.inner.wants.set(Want::Chat);
            } else {
                self.inner.wants.set(Want::List);
                self.inner.host.leave();
            }
        }
        true
    }

    /// Commits what the open modal shows, with the message a person settled on.
    ///
    /// `branch` non-empty commits onto a fresh branch of that name; `paths` non-empty takes
    /// only those files.
    pub fn commit_now(&self, subject: &str, body: &str, branch: &str, paths: Vec<String>) {
        let Some(opened) = self.inner.committing.get_untracked() else {
            return;
        };
        let subject = subject.trim();
        if subject.is_empty() {
            self.inner.host.say("the commit needs a message");
            return;
        }
        let message = if body.trim().is_empty() {
            subject.to_owned()
        } else {
            format!("{subject}\n\n{}", body.trim())
        };
        self.inner.client.commit(
            opened.root,
            opened.thread,
            message,
            opened.push,
            branch.trim().to_owned(),
            paths,
        );
        self.close_commit();
    }

    /// Makes a thread in a worktree of its own, branched from `base` in `root`'s repository.
    pub fn create_worktree_in(
        &self,
        root: std::path::PathBuf,
        base: String,
        from_origin: bool,
        instance: String,
    ) {
        self.inner
            .client
            .create_worktree(root, base, from_origin, instance);
    }
}

/// What the surface should show for the directory on screen.
#[derive(Clone, PartialEq, Debug)]
enum Answer {
    /// What is selected already works in it.
    Keep,
    /// This thread does, and is the one worked in last.
    Show(Box<ThreadShell>),
    /// It has none, and one is wanted.
    Make,
    /// It has none, and none is wanted. The chat shows nothing.
    Nothing,
}

/// Everything about the surface the rule reads, beside the threads themselves.
struct Asked {
    /// Which face the window shows.
    screen: Screen,
    /// Whether the daemon has said what threads there are.
    listed: bool,
    /// The directory a thread has already been asked for and not yet arrived in.
    asked_in: Option<std::path::PathBuf>,
}

/// The thread in `root` worked in last, when it has one.
///
/// Archived threads are put away and never come back on their own, so a directory whose only
/// threads are archived counts as one with none.
fn last_in(threads: &[ThreadShell], root: &std::path::Path) -> Option<ThreadShell> {
    threads
        .iter()
        .filter(|shell| !shell.archived && shell.root == root)
        .max_by_key(|shell| shell.updated_at_ms)
        .cloned()
}

/// What the surface should show for `root`.
///
/// Four answers, and which one applies is the whole rule:
///
/// - the selection already works in the directory: it stays. Choosing a thread by hand is never
///   undone by this;
/// - the directory has other threads: the one worked in last is what the chat shows;
/// - it has none and the chat is the screen: one is made, because a chat has to show a
///   conversation;
/// - it has none and the editor is the screen: nothing is selected. A thread from another
///   directory named here would answer for the wrong work and commit the wrong tree.
fn answer_for(
    root: &std::path::Path,
    selected: Option<&ThreadShell>,
    threads: &[ThreadShell],
    asked: Asked,
) -> Answer {
    if selected.is_some_and(|shell| shell.root == root) {
        return Answer::Keep;
    }
    if let Some(shell) = last_in(threads, root) {
        return Answer::Show(Box::new(shell));
    }
    // An empty list before the daemon has spoken says nothing at all, and a thread made on the
    // strength of it is a thread nobody asked for beside the ones already there.
    if asked.screen != Screen::Agent || !asked.listed {
        return Answer::Nothing;
    }
    // One ask at a time: the rows and the answer race, and asking again while the first is in
    // flight is a second thread nobody wanted.
    if asked.asked_in.as_deref() == Some(root) {
        return Answer::Nothing;
    }
    Answer::Make
}

/// How long ago `then` was, in one short word.
fn age_words(then: u64) -> String {
    let now = zdt_core::state::now_ms();
    let seconds = now.saturating_sub(then) / 1000;
    if seconds < 60 {
        "now".to_owned()
    } else if seconds < 3600 {
        format!("{}m ago", seconds / 60)
    } else if seconds < 86_400 {
        format!("{}h ago", seconds / 3600)
    } else {
        format!("{}d ago", seconds / 86_400)
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use zdt_agent::thread::{ThreadId, ThreadShell};

    use super::{Answer, Asked, Screen, answer_for, last_in};

    /// One thread in `root`, last moved at `updated`.
    fn shell(id: i64, root: &str, updated: u64) -> ThreadShell {
        ThreadShell {
            id: ThreadId(id),
            root: PathBuf::from(root),
            updated_at_ms: updated,
            ..ThreadShell::default()
        }
    }

    /// What the surface is like when the daemon has spoken and nothing is pending.
    fn asked(screen: Screen) -> Asked {
        Asked {
            screen,
            listed: true,
            asked_in: None,
        }
    }

    /// The directory the editor is showing in every test here.
    fn here() -> &'static Path {
        Path::new("/work/one")
    }

    #[test]
    fn a_directory_with_threads_shows_the_one_worked_in_last() {
        let threads = [
            shell(1, "/work/one", 100),
            shell(2, "/work/one", 300),
            shell(3, "/work/two", 900),
        ];
        for screen in [Screen::Editor, Screen::Agent] {
            let answer = answer_for(here(), None, &threads, asked(screen));
            assert_eq!(
                answer,
                Answer::Show(Box::new(threads[1].clone())),
                "{screen:?}"
            );
        }
    }

    #[test]
    fn a_directory_with_none_shows_nothing_in_the_editor_and_gets_one_in_the_chat() {
        // The whole point: switching sessions must not leave another directory's thread named
        // here, and the chat must never be a face with no conversation in it.
        let threads = [shell(3, "/work/two", 900)];
        assert_eq!(
            answer_for(here(), None, &threads, asked(Screen::Editor)),
            Answer::Nothing
        );
        assert_eq!(
            answer_for(here(), None, &threads, asked(Screen::Agent)),
            Answer::Make
        );
    }

    #[test]
    fn a_thread_from_another_directory_is_never_kept() {
        let elsewhere = shell(3, "/work/two", 900);
        let threads = [elsewhere.clone()];
        assert_eq!(
            answer_for(here(), Some(&elsewhere), &threads, asked(Screen::Editor)),
            Answer::Nothing
        );
    }

    #[test]
    fn a_selection_that_works_here_is_left_alone() {
        // Choosing a thread by hand is never undone, even when another one moved more recently.
        let chosen = shell(1, "/work/one", 100);
        let threads = [chosen.clone(), shell(2, "/work/one", 300)];
        assert_eq!(
            answer_for(here(), Some(&chosen), &threads, asked(Screen::Agent)),
            Answer::Keep
        );
    }

    #[test]
    fn nothing_is_made_before_the_daemon_has_spoken() {
        // An empty list means "not answered yet" until it says otherwise, and a thread made on
        // the strength of it is one nobody asked for.
        let asked = Asked {
            listed: false,
            ..asked(Screen::Agent)
        };
        assert_eq!(answer_for(here(), None, &[], asked), Answer::Nothing);
    }

    #[test]
    fn one_thread_is_asked_for_at_a_time() {
        // The rows and the answer race; asking again while the first is in flight is a second
        // thread nobody wanted.
        let asked = Asked {
            asked_in: Some(PathBuf::from("/work/one")),
            ..asked(Screen::Agent)
        };
        assert_eq!(answer_for(here(), None, &[], asked), Answer::Nothing);
    }

    #[test]
    fn an_archived_thread_is_not_a_thread_the_directory_has() {
        let mut put_away = shell(1, "/work/one", 900);
        put_away.archived = true;
        assert!(last_in(&[put_away.clone()], here()).is_none());
        assert_eq!(
            answer_for(here(), None, &[put_away], asked(Screen::Editor)),
            Answer::Nothing
        );
    }
}
