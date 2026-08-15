//! The pickers.
//!
//! One modal, several sources, and a rule that the interface thread never waits for any of them.
//!
//! # What happens between a keystroke and a row
//!
//! For a standing source — files, buffers, themes — the candidates are gathered once when the
//! picker opens, and every keystroke after that only re-ranks what is already in memory. The
//! ranking for the file list happens on `nucleo`'s own threads; this polls it a few times a second
//! from a timer and stops as soon as it says it has settled. Nothing on the interface thread ever
//! walks the candidate list.
//!
//! For a live source — grep — the query *is* the search. Each keystroke cancels the search before
//! it and starts another after a short pause, and hits arrive in batches so that the first ones are
//! on the screen long before the walk has finished.
//!
//! Both are guarded by a generation counter: an answer that arrives for a query nobody is asking
//! any more is dropped rather than drawn.

pub mod source;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use zdt_core::search::fuzzy::Ranker;
use zdt_core::search::{Cancel, Walk};
use zgui::reactive::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};

pub use crate::picker::source::{Reach, Row, Source, Target};
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
    /// Not asked for where it is used: a timer started from inside a task's continuation, or from
    /// inside another timer's callback, is outside the scope that has one, and asking there
    /// answers nothing at all. Taking it here — inside the root, where there certainly is one —
    /// is what makes the polling work wherever it is started from.
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
                candidates: RefCell::new(Vec::new()),
                ranker: RefCell::new(None),
                polling: RefCell::new(None),
                pending: RefCell::new(None),
                cancel: RefCell::new(None),
            }),
        }
    }

    // ---- What the interface reads ------------------------------------------------------------

    /// Which picker is open. Tracked.
    #[must_use]
    pub fn source(&self) -> Option<Source> {
        self.inner.source.get()
    }

    /// Whether one is. Tracked.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.inner.source.with(Option::is_some)
    }

    /// What has been typed. Tracked.
    #[must_use]
    pub fn query(&self) -> String {
        self.inner.query.get()
    }

    /// The rows. Tracked.
    #[must_use]
    pub fn rows(&self) -> Vec<Row> {
        self.inner.rows.get()
    }

    /// How many rows there are. Tracked, and narrower than reading them.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.rows.with(Vec::len)
    }

    /// Whether there are none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Which row the caret is on. Tracked.
    #[must_use]
    pub fn at(&self) -> usize {
        self.inner.at.get()
    }

    /// How many matched, and how many there were. Tracked.
    #[must_use]
    pub fn counts(&self) -> (usize, usize) {
        self.inner.counts.get()
    }

    /// Whether anything is still being gathered. Tracked.
    #[must_use]
    pub fn is_working(&self) -> bool {
        self.inner.working.get()
    }

    /// The row the caret is on.
    #[must_use]
    pub fn selected(&self) -> Option<Row> {
        self.inner
            .rows
            .with_untracked(|rows| rows.get(self.inner.at.get_untracked()).cloned())
    }

    // ---- Opening and closing -----------------------------------------------------------------

    /// Opens `source`, gathering whatever it needs.
    pub fn open(&self, source: Source) {
        self.stop();
        let start = source.start();
        self.inner.at.set(0);
        self.inner.rows.set(Vec::new());
        self.inner.counts.set((0, 0));
        self.inner.query.set(start.clone());
        self.inner.source.set(Some(source.clone()));
        self.gather(&source, &start);
    }

    /// Closes it, stopping whatever it had started.
    pub fn close(&self) {
        if self.inner.source.with_untracked(Option::is_none) {
            return;
        }
        self.stop();
        self.inner.source.set(None);
        self.inner.rows.set(Vec::new());
        self.inner.candidates.borrow_mut().clear();
        *self.inner.ranker.borrow_mut() = None;
        self.inner.workspace.focus_editor();
    }

    /// Stops every worker this picker has running, without closing it.
    fn stop(&self) {
        self.inner.generation.set(self.inner.generation.get() + 1);
        *self.inner.polling.borrow_mut() = None;
        *self.inner.pending.borrow_mut() = None;
        if let Some(cancel) = self.inner.cancel.borrow_mut().take() {
            cancel.stop();
        }
        self.inner.working.set(false);
    }

    // ---- Moving about ------------------------------------------------------------------------

    /// Moves the caret by `offset` rows, wrapping the way a picker does.
    pub fn move_by(&self, offset: isize) {
        let count = self.inner.rows.with_untracked(Vec::len);
        if count == 0 {
            return;
        }
        let at = self.inner.at.get_untracked() as isize + offset;
        let wrapped = at.rem_euclid(count as isize) as usize;
        self.inner.at.set(wrapped);
    }

    /// Puts the caret on `at`.
    pub fn go_to(&self, at: usize) {
        let count = self.inner.rows.with_untracked(Vec::len);
        if count > 0 {
            self.inner.at.set(at.min(count - 1));
        }
    }

    /// Takes what has been typed and searches or ranks again.
    pub fn set_query(&self, query: &str) {
        if self.inner.query.with_untracked(|held| held == query) {
            return;
        }
        self.inner.query.set(query.to_owned());
        let Some(source) = self.inner.source.get_untracked() else {
            return;
        };
        if source.is_live() {
            self.start_grep(&source, query);
        } else {
            self.rank(query);
        }
    }

    /// Does what the row the caret is on says, and closes.
    pub fn activate(&self) {
        let Some(row) = self.selected() else {
            return;
        };
        let workspace = self.inner.workspace.clone();
        self.close();

        match row.target {
            Target::File { path, line } => crate::files::open_at(&workspace, path, line),
            Target::Buffer(id) => workspace.show(id),
            Target::Line(line) => {
                if let Some(buffer) = workspace.current_buffer() {
                    crate::files::go_to(&workspace, buffer.id, line);
                }
            }
            Target::Theme(name) => {
                self.inner
                    .settings
                    .update(|config| config.ui.theme = name.clone());
                workspace.say(name);
            }
            Target::Action(action) => {
                if let Some(vim) = zgui::reactive::use_local_context::<crate::vim::Vim>() {
                    vim.run(&action);
                }
            }
            Target::Nothing => {}
        }
    }

    // ---- Gathering ---------------------------------------------------------------------------

    /// Puts whatever `source` picks from where the ranking can reach it.
    fn gather(&self, source: &Source, query: &str) {
        match source {
            Source::Files { reach } => self.gather_files(*reach),
            Source::Grep { .. } => self.start_grep(source, query),
            Source::Buffers => self.stand(self.buffers(), query),
            Source::Lines => self.stand(self.lines(), query),
            Source::Themes => self.stand(self.themes(), query),
            Source::Commands | Source::Keymaps => {
                self.stand(self.bindings(matches!(source, Source::Keymaps)), query);
            }
            Source::Recent => self.stand(self.recent(), query),
            Source::Registers => self.stand(self.registers(), query),
            Source::Marks => self.stand(self.marks(), query),
            Source::GitFiles => self.gather_git(query),
        }
    }

    /// Takes a gathered list and ranks it for the first time.
    fn stand(&self, rows: Vec<Row>, query: &str) {
        *self.inner.candidates.borrow_mut() = rows;
        self.rank(query);
    }

    /// The project's files, walked on a worker and handed to the matcher.
    fn gather_files(&self, reach: crate::picker::source::Reach) {
        let generation = self.inner.generation.get();
        let root = self.inner.workspace.project().root().to_path_buf();
        let (ignored, hidden) = self
            .inner
            .settings
            .with_untracked(|config| (config.picker.ignored, config.picker.hidden));
        let walk = Walk {
            ignored: reach.ignored || ignored,
            hidden: reach.hidden || hidden,
            ..Walk::default()
        };

        self.inner.working.set(true);
        let picker = self.clone();
        crate::task::detached(async move {
            let walked = {
                let root = root.clone();
                zgui::task::blocking(move || zdt_core::search::files::walk(&root, walk)).await
            };
            if picker.inner.generation.get() != generation {
                return;
            }

            // No wake: this polls on a timer of its own, so being told there is something
            // new would only ask for a frame that is coming anyway.
            let mut ranker = Ranker::new(|| {});
            ranker.fill(walked);
            // What has been typed *now*, not what had been when the walk started: a walk over a
            // large project takes long enough to type a word into, and that word must not be lost.
            ranker.seek(&picker.inner.query.get_untracked());
            *picker.inner.ranker.borrow_mut() = Some(ranker);
            picker.poll_ranker();
        });
    }

    /// Ranks the standing candidates, or asks the matcher to.
    fn rank(&self, query: &str) {
        // A file list still being walked has neither: leaving the rows alone is right, because
        // the walk reads what has been typed when it lands.
        if self.inner.ranker.borrow().is_none()
            && matches!(
                self.inner.source.get_untracked(),
                Some(Source::Files { .. } | Source::GitFiles)
            )
            && self.inner.candidates.borrow().is_empty()
        {
            return;
        }

        if self.inner.ranker.borrow().is_some() {
            if let Some(ranker) = self.inner.ranker.borrow_mut().as_mut() {
                ranker.seek(query);
            }
            self.poll_ranker();
            return;
        }

        let limit = self
            .inner
            .settings
            .with_untracked(|config| config.picker.max_results);
        let candidates = self.inner.candidates.borrow();
        let labels: Vec<String> = candidates.iter().map(|row| row.label.clone()).collect();
        let ranked = zdt_core::search::fuzzy::rank(&labels, query, limit);
        let rows: Vec<Row> = ranked
            .into_iter()
            .filter_map(|held| {
                candidates
                    .get(held.index)
                    .cloned()
                    .map(|row| row.with_matched(held.matched))
            })
            .collect();
        let total = candidates.len();
        drop(candidates);

        self.inner.counts.set((rows.len(), total));
        self.publish(rows);
    }

    /// Keeps asking the matcher for its answer until it says it has finished.
    fn poll_ranker(&self) {
        let Some(timers) = self.inner.timers.clone() else {
            return;
        };
        let generation = self.inner.generation.get();
        // The matcher says it has stopped before it has started: the items are pushed from here
        // and picked up by its threads a moment later, so the first tick answers "nothing to do"
        // about work that has not begun. A few quiet ticks, rather than one, is what tells the
        // difference between not started and finished.
        let quiet = Cell::new(0_u8);
        let picker = self.clone();
        let handle = timers.set_interval(POLL, move || {
            if picker.inner.generation.get() != generation {
                return;
            }
            let limit = picker
                .inner
                .settings
                .with_untracked(|config| config.picker.max_results);

            let (progress, matched, counts) = {
                let mut held = picker.inner.ranker.borrow_mut();
                let Some(ranker) = held.as_mut() else {
                    return;
                };
                let progress = ranker.poll();
                let matched = progress.changed.then(|| ranker.matches(limit));
                (progress, matched, ranker.counts())
            };

            let moved =
                picker.inner.counts.get_untracked() != (counts.0 as usize, counts.1 as usize);
            if let Some(matched) = matched.or_else(|| {
                // The counts moved without the matcher calling it a change: the items arrived.
                moved.then(|| {
                    let limit = picker
                        .inner
                        .settings
                        .with_untracked(|config| config.picker.max_results);
                    picker
                        .inner
                        .ranker
                        .borrow()
                        .as_ref()
                        .map(|ranker| ranker.matches(limit))
                        .unwrap_or_default()
                })
            }) {
                let root = picker.inner.workspace.project().root().to_path_buf();
                let rows: Vec<Row> = matched
                    .into_iter()
                    .map(|(path, landed)| Row::file(path, &root, None).with_matched(landed))
                    .collect();
                picker
                    .inner
                    .counts
                    .set((counts.0 as usize, counts.1 as usize));
                picker.publish(rows);
            }

            if progress.running || progress.changed || moved {
                quiet.set(0);
            } else {
                quiet.set(quiet.get() + 1);
            }
            if quiet.get() >= 4 {
                picker.inner.working.set(false);
                // Stopping from inside the callback: dropping the handle is what cancels it, and
                // the handle is held by the picker rather than by this closure.
                *picker.inner.polling.borrow_mut() = None;
            }
        });
        self.inner.working.set(true);
        *self.inner.polling.borrow_mut() = Some(handle);
    }

    // ---- Grep --------------------------------------------------------------------------------

    /// Starts a search after a pause, cancelling whatever was running.
    fn start_grep(&self, source: &Source, query: &str) {
        self.stop();
        self.inner.rows.set(Vec::new());
        self.inner.counts.set((0, 0));
        if query.is_empty() {
            return;
        }

        let Some(timers) = self.inner.timers.clone() else {
            return;
        };
        let reach = match source {
            Source::Grep { reach, .. } => *reach,
            _ => crate::picker::source::Reach::default(),
        };
        let query = query.to_owned();
        let picker = self.clone();
        let handle = timers.set_timeout(GREP_DEBOUNCE, move || picker.run_grep(reach, &query));
        *self.inner.pending.borrow_mut() = Some(handle);
        self.inner.working.set(true);
    }

    /// Runs one search, reporting its hits in batches.
    fn run_grep(&self, reach: crate::picker::source::Reach, pattern: &str) {
        let generation = self.inner.generation.get();
        let root = self.inner.workspace.project().root().to_path_buf();
        let (limit, smart_case) = self
            .inner
            .settings
            .with_untracked(|config| (config.picker.max_results, config.picker.smart_case));
        let query = zdt_core::search::grep::Query {
            pattern: pattern.to_owned(),
            regex: false,
            smart_case,
            walk: Walk {
                ignored: reach.ignored,
                hidden: reach.hidden,
                ..Walk::default()
            },
            limit: limit.max(1) * 4,
        };

        let cancel = Cancel::new();
        *self.inner.cancel.borrow_mut() = Some(cancel.clone());

        // Hits come back down a channel rather than through a posted closure: they are found on
        // the walk's own threads, and everything on this side of the picker is `Rc` and belongs to
        // the interface thread. A channel is the one shape that needs nothing of either.
        let (sender, receiver) = std::sync::mpsc::channel::<Vec<zdt_core::search::Hit>>();
        self.drain_hits(receiver, generation, limit, root.clone());

        let picker = self.clone();
        crate::task::detached(async move {
            let outcome = {
                let (root, query, cancel) = (root.clone(), query.clone(), cancel.clone());
                zgui::task::blocking(move || {
                    zdt_core::search::grep::search(&root, &query, &cancel, |batch| {
                        // A closed channel means nobody is listening any more, which is not worth
                        // saying: the search is about to be cancelled for the same reason.
                        let _ = sender.send(batch);
                    })
                })
                .await
            };
            if picker.inner.generation.get() != generation {
                return;
            }
            picker.inner.working.set(false);
            if let Err(error) = outcome {
                picker.inner.workspace.complain(error.to_string());
            }
        });
    }

    /// Takes whatever the search has found and puts it on the screen, a few times a second.
    ///
    /// Batched rather than drawn as each hit arrives: a grep over a large repository finds
    /// thousands, and a signal written per hit would be thousands of frames.
    fn drain_hits(
        &self,
        receiver: std::sync::mpsc::Receiver<Vec<zdt_core::search::Hit>>,
        generation: u64,
        limit: usize,
        root: std::path::PathBuf,
    ) {
        let Some(timers) = self.inner.timers.clone() else {
            return;
        };
        let picker = self.clone();
        let handle = timers.set_interval(POLL, move || {
            if picker.inner.generation.get() != generation {
                return;
            }
            let rows: Vec<Row> = receiver
                .try_iter()
                .flatten()
                .map(|hit| {
                    Row::file(hit.path, &root, Some(hit.line))
                        .with_detail(hit.text.trim_start().to_owned())
                })
                .collect();
            picker.extend(rows, limit);

            // The walk has finished and the channel is empty: there is nothing left to drain.
            if !picker.inner.working.get_untracked() {
                *picker.inner.polling.borrow_mut() = None;
            }
        });
        *self.inner.polling.borrow_mut() = Some(handle);
    }

    // ---- Publishing --------------------------------------------------------------------------

    /// Puts `rows` where the list reads them, keeping the caret in range.
    fn publish(&self, rows: Vec<Row>) {
        let count = rows.len();
        self.inner.rows.set(rows);
        let at = self.inner.at.get_untracked();
        if count == 0 {
            if at != 0 {
                self.inner.at.set(0);
            }
        } else if at >= count {
            self.inner.at.set(count - 1);
        }
    }

    /// Adds `rows` to what is already shown, up to `limit`.
    ///
    /// What a live source does: the first hits are drawn while the rest are still being found.
    fn extend(&self, rows: Vec<Row>, limit: usize) {
        if rows.is_empty() {
            return;
        }
        let mut held = self.inner.rows.get_untracked();
        if held.len() >= limit {
            return;
        }
        let room = limit - held.len();
        held.extend(rows.into_iter().take(room));
        let count = held.len();
        self.inner.rows.set(held);
        self.inner.counts.set((count, count));
    }

    // ---- The standing lists ------------------------------------------------------------------

    /// The open buffers, the one being edited last — it is the one somebody is switching away
    /// from, so it is the least likely thing they are switching to.
    fn buffers(&self) -> Vec<Row> {
        let current = self
            .inner
            .workspace
            .current_buffer()
            .map(|buffer| buffer.id);
        let mut rows: Vec<Row> = Vec::new();
        for id in self.inner.workspace.order() {
            let Some(buffer) = self.inner.workspace.buffer_untracked(id) else {
                continue;
            };
            let label = match &buffer.path {
                Some(path) => self.inner.workspace.project().relative(path).into_owned(),
                None => "[no name]".to_owned(),
            };
            let kind = buffer
                .path
                .as_deref()
                .map(zdt_core::language::of)
                .unwrap_or(zdt_core::language::UNKNOWN);
            let row = Row {
                label,
                detail: if buffer.is_dirty() {
                    "modified".to_owned()
                } else {
                    String::new()
                },
                matched: Vec::new(),
                glyph: Some(kind.glyph),
                tint: Some(kind.tint),
                target: Target::Buffer(id),
            };
            if Some(id) == current {
                rows.push(row);
            } else {
                rows.insert(0, row);
            }
        }
        rows.reverse();
        rows
    }

    /// The files opened this session, the most recent first, leaving out the ones still open.
    ///
    /// A file that is open is one keystroke away on the buffer line; a recent-files list that
    /// repeated it would be spending its rows on the answer somebody already has.
    fn recent(&self) -> Vec<Row> {
        let root = self.inner.workspace.project().root().to_path_buf();
        let open: Vec<std::path::PathBuf> = self
            .inner
            .workspace
            .order()
            .into_iter()
            .filter_map(|id| self.inner.workspace.buffer_untracked(id))
            .filter_map(|buffer| buffer.path)
            .collect();

        self.inner
            .workspace
            .recent()
            .into_iter()
            .filter(|path| !open.contains(path))
            .map(|path| {
                let shown = self.inner.workspace.project().relative(&path).into_owned();
                Row::file(shown, &root, None)
            })
            .collect()
    }

    /// What is in each register, as one row each.
    fn registers(&self) -> Vec<Row> {
        let Some(vim) = zgui::reactive::use_local_context::<crate::vim::Vim>() else {
            return Vec::new();
        };
        vim.registers()
            .into_iter()
            .map(|(name, text)| {
                // One line of it: a register holding forty lines is still one row here, and the
                // first line is the part that says which one it is.
                let first = text.lines().next().unwrap_or("").trim_end();
                Row::plain(format!("\"{name}"), Target::Nothing).with_detail(first.to_owned())
            })
            .collect()
    }

    /// Where each mark is, as the line it sits on.
    fn marks(&self) -> Vec<Row> {
        let Some(vim) = zgui::reactive::use_local_context::<crate::vim::Vim>() else {
            return Vec::new();
        };
        let Some(handle) = self.inner.workspace.current_handle() else {
            return Vec::new();
        };

        handle.query(|snapshot| {
            let rope = snapshot.rope();
            vim.marks()
                .into_iter()
                .map(|(name, byte)| {
                    let byte = byte.min(rope.len_bytes());
                    let line = rope.byte_to_line(byte);
                    let text = rope
                        .line(line)
                        .to_string()
                        .trim_end_matches(['\n', '\r'])
                        .trim_start()
                        .to_owned();
                    Row::plain(format!("'{name}"), Target::Line(line as u64 + 1))
                        .with_detail(format!("{}  {text}", line + 1))
                })
                .collect()
        })
    }

    /// The files git is tracking, which is the file list minus everything untracked.
    fn gather_git(&self, query: &str) {
        let generation = self.inner.generation.get();
        let root = self.inner.workspace.project().root().to_path_buf();
        let picker = self.clone();
        let query = query.to_owned();

        self.inner.working.set(true);
        crate::task::detached(async move {
            let listed = {
                let root = root.clone();
                zgui::task::blocking(move || git_files(&root)).await
            };
            if picker.inner.generation.get() != generation {
                return;
            }
            picker.inner.working.set(false);
            let rows = listed
                .into_iter()
                .map(|path| Row::file(path, &root, None))
                .collect();
            picker.stand(rows, &query);
        });
    }

    /// The lines of the buffer being edited.
    fn lines(&self) -> Vec<Row> {
        let Some(handle) = self.inner.workspace.current_handle() else {
            return Vec::new();
        };
        handle.query(|snapshot| {
            let rope = snapshot.rope();
            rope.lines()
                .enumerate()
                .map(|(index, line)| {
                    let text = line.to_string();
                    Row::plain(
                        text.trim_end_matches(['\n', '\r']).to_owned(),
                        Target::Line(index as u64 + 1),
                    )
                    .with_detail(format!("{}", index + 1))
                })
                .collect()
        })
    }

    /// The themes there are, the built-in ones and whatever is in the configuration directory.
    fn themes(&self) -> Vec<Row> {
        let directory = self.inner.settings.paths().map(|paths| paths.themes());
        zdt_core::theme::theme_names(directory.as_deref())
            .into_iter()
            .map(|name| Row::plain(name.clone(), Target::Theme(name)))
            .collect()
    }

    /// Everything the keymap can do.
    ///
    /// As commands, it is one row per description, so the same thing bound twice reads once. As
    /// keys, it is one row per binding, because which key does it is the question being asked.
    fn bindings(&self, by_key: bool) -> Vec<Row> {
        let Some(vim) = zgui::reactive::use_local_context::<crate::vim::Vim>() else {
            return Vec::new();
        };
        let mut rows: Vec<Row> = Vec::new();
        let mut seen: rustc_hash::FxHashSet<String> = rustc_hash::FxHashSet::default();

        for bound in vim.bindings() {
            let described = if bound.description.is_empty() {
                bound
                    .actions
                    .first()
                    .map_or(String::new(), |action| action.name.replace(['.', '_'], " "))
            } else {
                bound.description.clone()
            };
            let Some(action) = bound.actions.first().cloned() else {
                continue;
            };

            if by_key {
                rows.push(
                    Row::plain(bound.keys.clone(), Target::Action(action)).with_detail(described),
                );
            } else if seen.insert(described.clone()) {
                rows.push(Row::plain(described, Target::Action(action)).with_detail(bound.keys));
            }
        }
        rows.sort_by(|left, right| left.label.cmp(&right.label));
        rows
    }
}

/// What `git ls-files` says, or nothing when this is not a repository.
///
/// Blocking, and a process rather than a library: the answer is a list of names, git already
/// knows it, and linking a git implementation to ask would be a large dependency for one list.
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
