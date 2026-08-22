//! Carrying files from one directory to another with the pointer.
//!
//! A press is watched before it is believed. Nothing happens until the pointer has travelled
//! [`THRESHOLD`] pixels, so a click whose hand shook stays a click, and only then is anything
//! lifted.
//!
//! The state is three signals rather than one, and the split is what keeps a drag cheap. Where the
//! pointer is changes on every frame and only the ghost reads it. Where a drop would land changes a
//! few times in a whole gesture and every visible row reads it. One signal holding both would wake
//! forty rows sixty times a second.

pub mod ghost;

use std::path::{Path, PathBuf};

use zdt_core::tree::Row;
use zgui::geom::{Css, CssPx, Point};
use zgui::reactive::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};

/// How far a press travels before it is a drag, in CSS pixels.
///
/// A mouse is precise, so this separates a click whose hand shook from a deliberate movement rather
/// than making room for a fingertip. It is the same distance the framework's own gesture code uses.
pub const THRESHOLD: f32 = 4.0;

/// How many of the rows going with a drag the ghost draws before it counts the rest.
pub const SHOWN: usize = 3;

/// What a drag is carrying.
#[derive(Clone, PartialEq, Debug)]
pub struct Carrying {
    /// Everything that would move.
    pub paths: Vec<PathBuf>,
    /// The row that was pressed. What the ghost draws, and what a refused drop flies back to.
    pub row: usize,
    /// Where the press was, in CSS pixels from the window's top-left corner.
    pub from: Point<CssPx, Css>,
    /// Where the ghost's own top-left corner sits, so it stays under the part that was grabbed.
    pub grab: Point<CssPx, Css>,
}

/// Where a drop would land.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub enum Landing {
    /// Nowhere: outside the list, or nothing carried may go where the pointer is.
    #[default]
    Nowhere,
    /// Into the directory the tree is rooted at.
    Root,
    /// Into this directory.
    Into(PathBuf),
}

impl Landing {
    /// The directory that would receive the drop, when a row on screen is one.
    #[must_use]
    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::Into(path) => Some(path),
            Self::Nowhere | Self::Root => None,
        }
    }

    /// Whether a drop here would do anything.
    #[must_use]
    pub const fn accepts(&self) -> bool {
        !matches!(self, Self::Nowhere)
    }
}

/// How far a press has got.
#[derive(Clone, PartialEq, Debug, Default)]
enum Phase {
    /// No press is being watched.
    #[default]
    Idle,
    /// A press that has not travelled far enough to be a drag.
    Armed(Carrying),
    /// A drag.
    Lifted(Carrying),
}

/// The pointer gesture that moves files.
///
/// Cloning one is cloning a handle: every signal in it is `Copy`.
#[derive(Clone, Copy)]
pub struct Drag {
    /// The press being watched, and the drag it became.
    phase: RwSignal<Phase, LocalStorage>,
    /// Where the pointer is now. Only the ghost reads this.
    at: RwSignal<Point<CssPx, Css>, LocalStorage>,
    /// Where a drop would land. Every visible row reads this.
    landing: RwSignal<Landing, LocalStorage>,
    /// Where the ghost flies to when it is let go, while it is flying.
    settling: RwSignal<Option<Point<CssPx, Css>>, LocalStorage>,
}

impl Default for Drag {
    fn default() -> Self {
        Self::new()
    }
}

impl Drag {
    /// Nothing being pressed and nothing being carried.
    #[must_use]
    pub fn new() -> Self {
        Self {
            phase: RwSignal::new_local(Phase::Idle),
            at: RwSignal::new_local(origin()),
            landing: RwSignal::new_local(Landing::Nowhere),
            settling: RwSignal::new_local(None),
        }
    }

    // ---- What the pointer does ---------------------------------------------------------------

    /// Watches a press on the row at `row`, which may become a drag of `paths`.
    ///
    /// `at` is where the pointer went down and `row_top` where the row it landed on starts, so that
    /// the ghost can be held at the point it was taken hold of.
    pub fn arm(&self, paths: Vec<PathBuf>, row: usize, at: Point<CssPx, Css>, row_top: f32) {
        if paths.is_empty() {
            return;
        }
        let grab = Point::new(CssPx(at.x.0 - GRAB_X), CssPx(row_top));
        self.phase.set(Phase::Armed(Carrying {
            paths,
            row,
            from: at,
            grab,
        }));
        self.at.set(at);
    }

    /// Records that the pointer moved to `at`, over the directory `landing` would receive.
    ///
    /// Answers `true` on the one move that lifts the press into a drag, which is when the pointer
    /// is captured.
    pub fn moved(&self, at: Point<CssPx, Css>, landing: Landing) -> bool {
        let lifting = self.phase.with_untracked(|phase| match phase {
            Phase::Armed(held) => past_threshold(held.from, at),
            Phase::Idle | Phase::Lifted(_) => false,
        });
        if lifting {
            self.phase
                .update(|phase| take_if_armed(phase, Phase::Lifted));
        } else if !self.is_lifted_untracked() {
            return false;
        }
        self.at.set(at);
        if self.landing.with_untracked(|was| *was != landing) {
            self.landing.set(landing);
        }
        lifting
    }

    /// Ends a drag by letting go, answering what should move where.
    ///
    /// The ghost is left flying to `to`, which is where the rows land. Answers nothing when the
    /// drop was refused; the caller then springs it back.
    pub fn land(&self, to: Point<CssPx, Css>, root: &Path) -> Option<(Vec<PathBuf>, PathBuf)> {
        let held = self.lifted()?;
        let into = match self.landing.get_untracked() {
            Landing::Into(path) => path,
            Landing::Root => root.to_path_buf(),
            Landing::Nowhere => return None,
        };
        let moving = movable(&held.paths, &into);
        if moving.is_empty() {
            return None;
        }
        self.settle(to);
        Some((moving, into))
    }

    /// Ends a drag without moving anything, flying the ghost back to where it was taken from.
    pub fn spring_back(&self, to: Option<Point<CssPx, Css>>) {
        let Some(held) = self.lifted() else {
            self.disarm();
            return;
        };
        self.settle(to.unwrap_or(held.grab));
    }

    /// Forgets a press that never became a drag, answering the row it was on.
    pub fn disarm(&self) -> Option<usize> {
        let mut row = None;
        self.phase.update(|phase| {
            if let Phase::Armed(held) = phase {
                row = Some(held.row);
                *phase = Phase::Idle;
            }
        });
        row
    }

    /// Says the ghost has finished flying and may be taken away.
    pub fn settled(&self) {
        if self.settling.with_untracked(Option::is_some) {
            self.settling.set(None);
        }
    }

    // ---- What the interface reads ------------------------------------------------------------

    /// What is being carried, while something is. Tracked.
    #[must_use]
    pub fn carrying(&self) -> Option<Carrying> {
        self.phase.with(|phase| match phase {
            Phase::Lifted(held) => Some(held.clone()),
            Phase::Idle | Phase::Armed(_) => None,
        })
    }

    /// What is being carried, without subscribing.
    #[must_use]
    pub fn carrying_untracked(&self) -> Option<Carrying> {
        self.lifted()
    }

    /// Whether a press is being watched to see whether it becomes a drag, without subscribing.
    #[must_use]
    pub fn is_armed_untracked(&self) -> bool {
        self.phase
            .with_untracked(|phase| matches!(phase, Phase::Armed(_)))
    }

    /// Whether something is being dragged, as opposed to merely pressed. Tracked.
    #[must_use]
    pub fn is_lifted(&self) -> bool {
        self.phase.with(|phase| matches!(phase, Phase::Lifted(_)))
    }

    /// The same, without subscribing.
    #[must_use]
    pub fn is_lifted_untracked(&self) -> bool {
        self.phase
            .with_untracked(|phase| matches!(phase, Phase::Lifted(_)))
    }

    /// Whether `path` is one of the things in flight. Tracked.
    #[must_use]
    pub fn carries(&self, path: &Path) -> bool {
        self.phase.with(|phase| match phase {
            Phase::Lifted(held) => held.paths.iter().any(|held| held == path),
            Phase::Idle | Phase::Armed(_) => false,
        })
    }

    /// Where a drop would land. Tracked.
    #[must_use]
    pub fn landing(&self) -> Landing {
        self.landing.get()
    }

    /// Where the pointer is. Tracked.
    #[must_use]
    pub fn at(&self) -> Point<CssPx, Css> {
        self.at.get()
    }

    /// Where the pointer is, without subscribing.
    #[must_use]
    pub fn at_untracked(&self) -> Point<CssPx, Css> {
        self.at.get_untracked()
    }

    /// Where the ghost is flying to, while it is flying. Tracked.
    #[must_use]
    pub fn settling(&self) -> Option<Point<CssPx, Css>> {
        self.settling.get()
    }

    // ---- Internals ---------------------------------------------------------------------------

    /// What is being carried, without subscribing.
    fn lifted(&self) -> Option<Carrying> {
        self.phase.with_untracked(|phase| match phase {
            Phase::Lifted(held) => Some(held.clone()),
            Phase::Idle | Phase::Armed(_) => None,
        })
    }

    /// Ends the gesture and starts the ghost's flight to `to`.
    fn settle(&self, to: Point<CssPx, Css>) {
        self.phase.set(Phase::Idle);
        self.landing.set(Landing::Nowhere);
        self.settling.set(Some(to));
    }
}

/// Where the ghost's leading edge sits, relative to the pointer.
///
/// A little to the left, so the pointer falls on the ghost's own left margin and the name being
/// carried is readable beside it rather than under the cursor.
const GRAB_X: f32 = 10.0;

/// The window's top-left corner.
fn origin() -> Point<CssPx, Css> {
    Point::new(CssPx(0.0), CssPx(0.0))
}

/// Whether a pointer that pressed at `from` and is now at `to` has travelled far enough to drag.
#[must_use]
pub fn past_threshold(from: Point<CssPx, Css>, to: Point<CssPx, Css>) -> bool {
    let dx = to.x.0 - from.x.0;
    let dy = to.y.0 - from.y.0;
    dx * dx + dy * dy > THRESHOLD * THRESHOLD
}

/// Replaces an armed press with whatever `become_` makes of it.
fn take_if_armed(phase: &mut Phase, become_: impl FnOnce(Carrying) -> Phase) {
    if let Phase::Armed(held) = std::mem::take(phase) {
        *phase = become_(held);
    }
}

/// The directory a drop on `row` would land in, when anything carried may go there.
///
/// A directory receives into itself, and a file into the directory holding it: dropping *beside*
/// something means dropping into what holds it. `None` for `row` is the space past the last row,
/// which is the root.
///
/// A path is refused when it is the target, when the target already holds it, or when the target is
/// inside it — a directory moved into itself would take the tree with it. The drop is allowed while
/// one carried path survives all three, and only those move.
#[must_use]
pub fn landing_for(row: Option<&Row>, carried: &[PathBuf], root: &Path) -> Landing {
    let into = match row {
        Some(row) if row.entry.directory => row.entry.path.clone(),
        Some(row) => match row.entry.path.parent() {
            Some(parent) => parent.to_path_buf(),
            None => return Landing::Nowhere,
        },
        None => root.to_path_buf(),
    };
    if movable(carried, &into).is_empty() {
        return Landing::Nowhere;
    }
    if into == root {
        Landing::Root
    } else {
        Landing::Into(into)
    }
}

/// Where a drag that came to nothing flies back to: the row it started on, where that row is *now*.
///
/// Read again rather than remembered, because the list may have scrolled under the pointer while
/// the drag was in flight and the file is wherever the row is.
#[must_use]
pub fn home(explorer: &crate::explorer::Explorer) -> Option<Point<CssPx, Css>> {
    let held = explorer.drag().carrying_untracked()?;
    let rect = explorer.viewport()?.row_rect(held.row)?;
    Some(Point::new(CssPx(rect.x), CssPx(rect.y)))
}

/// Which of `carried` may move into `into`.
#[must_use]
pub fn movable(carried: &[PathBuf], into: &Path) -> Vec<PathBuf> {
    carried
        .iter()
        .filter(|path| path.as_path() != into)
        .filter(|path| path.parent() != Some(into))
        .filter(|path| !into.starts_with(path))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use zdt_core::tree::{Entry, Row, Standing};
    use zgui::geom::{Css, CssPx, Point};

    use super::{Drag, Landing, landing_for, movable, past_threshold};

    fn at(x: f32, y: f32) -> Point<CssPx, Css> {
        Point::new(CssPx(x), CssPx(y))
    }

    fn row(path: &str, directory: bool) -> Row {
        Row {
            entry: Entry {
                path: PathBuf::from(path),
                name: Path::new(path)
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                directory,
                standing: Standing::default(),
            },
            depth: 1,
            expanded: false,
        }
    }

    fn paths(of: &[&str]) -> Vec<PathBuf> {
        of.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn a_click_whose_hand_shook_is_still_a_click() {
        assert!(!past_threshold(at(0.0, 0.0), at(2.0, 2.0)));
        assert!(!past_threshold(at(0.0, 0.0), at(0.0, 4.0)));
    }

    #[test]
    fn a_deliberate_movement_is_a_drag() {
        assert!(past_threshold(at(0.0, 0.0), at(0.0, 5.0)));
        assert!(past_threshold(at(20.0, 20.0), at(0.0, 20.0)));
    }

    #[test]
    fn a_drop_on_a_directory_lands_in_it() {
        let held = paths(&["/p/a.rs"]);
        assert_eq!(
            landing_for(Some(&row("/p/src", true)), &held, Path::new("/p")),
            Landing::Into(PathBuf::from("/p/src"))
        );
    }

    #[test]
    fn a_drop_on_a_file_lands_in_the_directory_holding_it() {
        let held = paths(&["/p/a.rs"]);
        assert_eq!(
            landing_for(Some(&row("/p/src/main.rs", false)), &held, Path::new("/p")),
            Landing::Into(PathBuf::from("/p/src"))
        );
    }

    #[test]
    fn a_drop_past_the_last_row_lands_in_the_root() {
        let held = paths(&["/p/src/a.rs"]);
        assert_eq!(landing_for(None, &held, Path::new("/p")), Landing::Root);
    }

    #[test]
    fn the_directory_a_file_already_sits_in_refuses_it() {
        let held = paths(&["/p/src/a.rs"]);
        assert_eq!(
            landing_for(Some(&row("/p/src", true)), &held, Path::new("/p")),
            Landing::Nowhere
        );
    }

    #[test]
    fn a_directory_refuses_itself_and_everything_inside_it() {
        let held = paths(&["/p/src"]);
        assert_eq!(
            landing_for(Some(&row("/p/src", true)), &held, Path::new("/p")),
            Landing::Nowhere,
            "a directory dropped on itself would move nowhere"
        );
        assert_eq!(
            landing_for(Some(&row("/p/src/core", true)), &held, Path::new("/p")),
            Landing::Nowhere,
            "a directory moved inside itself would take the tree with it"
        );
    }

    #[test]
    fn a_mixed_set_keeps_only_what_may_move() {
        let held = paths(&["/p/src/a.rs", "/p/b.rs"]);
        assert_eq!(
            landing_for(Some(&row("/p/src", true)), &held, Path::new("/p")),
            Landing::Into(PathBuf::from("/p/src")),
            "one of the two is already there, and the other is not"
        );
        assert_eq!(movable(&held, Path::new("/p/src")), paths(&["/p/b.rs"]));
    }

    #[test]
    fn a_press_that_never_travelled_lifts_nothing() {
        let drag = Drag::new();
        drag.arm(paths(&["/p/a.rs"]), 3, at(10.0, 10.0), 8.0);
        assert!(!drag.moved(at(11.0, 11.0), Landing::Nowhere));
        assert!(!drag.is_lifted_untracked());
        assert_eq!(drag.disarm(), Some(3));
    }

    #[test]
    fn the_lift_is_reported_exactly_once() {
        let drag = Drag::new();
        drag.arm(paths(&["/p/a.rs"]), 0, at(10.0, 10.0), 8.0);
        assert!(drag.moved(at(10.0, 40.0), Landing::Nowhere));
        assert!(
            !drag.moved(at(10.0, 60.0), Landing::Nowhere),
            "every further move would otherwise capture the pointer again"
        );
        assert!(drag.is_lifted_untracked());
    }

    #[test]
    fn a_refused_drop_moves_nothing() {
        let drag = Drag::new();
        drag.arm(paths(&["/p/src/a.rs"]), 0, at(10.0, 10.0), 8.0);
        drag.moved(at(10.0, 40.0), Landing::Nowhere);
        assert_eq!(drag.land(at(10.0, 40.0), Path::new("/p")), None);
    }

    #[test]
    fn a_drop_answers_what_moves_and_where() {
        let drag = Drag::new();
        drag.arm(paths(&["/p/a.rs"]), 0, at(10.0, 10.0), 8.0);
        drag.moved(at(10.0, 40.0), Landing::Into(PathBuf::from("/p/src")));
        assert_eq!(
            drag.land(at(10.0, 40.0), Path::new("/p")),
            Some((paths(&["/p/a.rs"]), PathBuf::from("/p/src")))
        );
        assert!(!drag.is_lifted_untracked(), "the gesture is over");
    }
}
