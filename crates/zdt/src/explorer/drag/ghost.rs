//! What the pointer carries while a drag is in flight.
//!
//! One element, mounted for the life of the window and hidden by an attribute. Mounting a node
//! changes the document, and the frame a drag begins on is the worst one to spend on that.
//!
//! It moves by a transform and never by its own `left` and `top`. A transform is handed straight to
//! the fragment pass, so following the pointer costs a repaint; a length would be a full cascade of
//! the element on every frame of the gesture.

use zdt_core::tree::Row;
use zgui::geom::{CssPx, Point};
use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};
use zgui::{component, view};

use crate::explorer::drag::{Carrying, SHOWN};
use crate::explorer::use_explorer;

/// The rows under the pointer.
#[component]
pub fn TreeGhost() -> impl IntoView {
    let explorer = use_explorer();
    let drag = explorer.drag();

    // What was being carried, kept for the length of the flight: the gesture ends the moment the
    // button comes up, and a ghost that read the drag directly would empty as it left.
    let showing: RwSignal<Option<Carrying>, LocalStorage> = RwSignal::new_local(None);
    let follow = zgui::reactive::RenderEffect::new(move |_| {
        if let Some(held) = drag.carrying() {
            showing.set(Some(held));
        }
    });
    on_cleanup_local(move || drop(follow));

    let state = move || {
        Some(
            if drag.is_lifted() {
                "on"
            } else if drag.settling().is_some() {
                "settling"
            } else {
                "off"
            }
            .to_owned(),
        )
    };
    let refused =
        move || (drag.is_lifted() && !drag.landing().accepts()).then(|| "true".to_owned());

    // Where it has moved to, from where it started. While the pointer holds it, that is how far the
    // pointer has come; once it is let go, it is the whole way to where the rows land.
    let offset = move || {
        let held = showing.get()?;
        let to = drag.settling().unwrap_or_else(|| {
            let at = drag.at();
            Point::new(
                CssPx(held.grab.x.0 + at.x.0 - held.from.x.0),
                CssPx(held.grab.y.0 + at.y.0 - held.from.y.0),
            )
        });
        Some((to.x.0 - held.grab.x.0, to.y.0 - held.grab.y.0))
    };
    let ghost_x = { move || offset().map(|(x, _)| format!("{x:.1}px")) };
    let ghost_y = move || offset().map(|(_, y)| format!("{y:.1}px"));
    let left = move || showing.get().map(|held| format!("{:.1}px", held.grab.x.0));
    let top = move || showing.get().map(|held| format!("{:.1}px", held.grab.y.0));

    let carried = {
        let explorer = explorer.clone();
        move || {
            let held = showing.get()?;
            Some(carried_rows(&explorer.rows(), &held))
        }
    };
    let lines = {
        let carried = carried.clone();
        move || {
            use zdt_view::Erase;
            carried()
                .map(|(rows, _)| rows)
                .unwrap_or_default()
                .into_iter()
                .enumerate()
                .map(|(at, row)| view! { Line(row = row, along = at > 0) }.any())
                .collect::<Vec<_>>()
        }
    };
    let more = {
        let carried = carried.clone();
        move || match carried() {
            Some((_, left)) if left > 0 => format!("+{left}"),
            _ => String::new(),
        }
    };
    let more_on = move || {
        carried()
            .is_some_and(|(_, left)| left > 0)
            .then(|| "true".to_owned())
    };

    view! {
        column(
            class = "treeghost",
            attr:data-state = state,
            attr:data-refused = refused,
            var:--ghost-x = ghost_x,
            var:--ghost-y = ghost_y,
            style:left = left,
            style:top = top,
            style:max-width = {
                let explorer = explorer.clone();
                move || Some(explorer.width().inset_px(16))
            },
            a11y:hidden = true,
            on:transition_end = move |_: &mut EventCx<'_, events::TransitionEnd>| drag.settled()
        ) {
            {lines}
            label(class = "treeghost__more", attr:data-on = more_on) {{more}}
        }
    }
}

/// One row in the ghost.
#[component]
fn Line(
    /// What it stands for.
    row: Row,
    /// Whether it is one of the rows coming along, rather than the one that was taken hold of.
    along: bool,
) -> impl IntoView {
    let kind = row.entry.file_type();
    let glyph = if row.entry.directory {
        zdt_core::language::DIRECTORY_OPEN.glyph
    } else {
        kind.glyph
    };
    view! {
        row(class = "treeghost__line", attr:data-along = along.then(|| "true".to_owned())) {
            label(class = "glyph", style:color = Some(format!("var(--{})", kind.tint))) {{glyph}}
            label(class = "treeghost__name nowrap") {{row.entry.name.clone()}}
        }
    }
}

/// The rows the ghost draws, and how many more are moving than it has room for.
///
/// One file is itself. One directory is itself and what the tree has read of its contents, so that
/// a directory is seen to travel with what is in it. Several picked-out rows are themselves.
///
/// The count is only ever of rows the tree already holds. A directory nobody has opened is drawn
/// alone rather than with a guess at what is in it.
fn carried_rows(rows: &[Row], held: &Carrying) -> (Vec<Row>, usize) {
    let mut moving: Vec<Row> = if held.paths.len() > 1 {
        // The row that was taken hold of goes on top, wherever it sits in the set. The ghost hangs
        // from the pointer, and the name under the pointer has to be the one that was grabbed.
        let mut moving: Vec<Row> = held
            .paths
            .iter()
            .filter_map(|path| rows.iter().find(|row| row.entry.path == *path).cloned())
            .collect();
        if let Some(at) = rows.get(held.row).and_then(|row| {
            moving
                .iter()
                .position(|held| held.entry.path == row.entry.path)
        }) {
            moving.swap(0, at);
        }
        moving
    } else {
        let Some(at) = rows.iter().position(|row| row.entry.path == held.paths[0]) else {
            return (Vec::new(), 0);
        };
        let mut moving = vec![rows[at].clone()];
        moving.extend(
            rows[at + 1..]
                .iter()
                .take_while(|row| row.depth > rows[at].depth)
                .cloned(),
        );
        moving
    };
    let over = moving.len().saturating_sub(SHOWN + 1);
    moving.truncate(SHOWN + 1);
    (moving, over)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use zdt_core::tree::{Entry, Row, Standing};
    use zgui::geom::{CssPx, Point};

    use super::carried_rows;
    use crate::explorer::drag::Carrying;

    fn row(path: &str, directory: bool, depth: usize) -> Row {
        Row {
            entry: Entry {
                path: PathBuf::from(path),
                name: path.rsplit('/').next().unwrap_or_default().to_owned(),
                directory,
                standing: Standing::default(),
            },
            depth,
            expanded: directory,
        }
    }

    fn held(paths: &[&str]) -> Carrying {
        Carrying {
            paths: paths.iter().map(PathBuf::from).collect(),
            row: 0,
            from: Point::new(CssPx(0.0), CssPx(0.0)),
            grab: Point::new(CssPx(0.0), CssPx(0.0)),
        }
    }

    #[test]
    fn a_file_travels_alone() {
        let rows = vec![row("/p/a.rs", false, 1), row("/p/b.rs", false, 1)];
        let (drawn, over) = carried_rows(&rows, &held(&["/p/a.rs"]));
        assert_eq!(drawn.len(), 1);
        assert_eq!(over, 0);
    }

    #[test]
    fn a_directory_travels_with_what_is_in_it() {
        let rows = vec![
            row("/p/src", true, 1),
            row("/p/src/a.rs", false, 2),
            row("/p/src/b.rs", false, 2),
            row("/p/other.rs", false, 1),
        ];
        let (drawn, over) = carried_rows(&rows, &held(&["/p/src"]));
        assert_eq!(drawn.len(), 3, "the directory and the two files under it");
        assert_eq!(over, 0);
        assert!(!drawn.iter().any(|row| row.entry.path.ends_with("other.rs")));
    }

    #[test]
    fn what_will_not_fit_is_counted() {
        let mut rows = vec![row("/p/src", true, 1)];
        rows.extend((0..9).map(|at| row(&format!("/p/src/{at}.rs"), false, 2)));
        let (drawn, over) = carried_rows(&rows, &held(&["/p/src"]));
        assert_eq!(drawn.len(), 4);
        assert_eq!(over, 6);
    }

    #[test]
    fn a_set_travels_as_itself() {
        let rows = vec![
            row("/p/a.rs", false, 1),
            row("/p/b.rs", false, 1),
            row("/p/c.rs", false, 1),
        ];
        let (drawn, over) = carried_rows(&rows, &held(&["/p/a.rs", "/p/c.rs"]));
        assert_eq!(drawn.len(), 2);
        assert_eq!(over, 0);
    }

    #[test]
    fn the_row_that_was_grabbed_is_the_one_under_the_pointer() {
        let rows = vec![
            row("/p/a.rs", false, 1),
            row("/p/b.rs", false, 1),
            row("/p/c.rs", false, 1),
        ];
        let mut grabbed = held(&["/p/a.rs", "/p/c.rs"]);
        grabbed.row = 2;
        let (drawn, _) = carried_rows(&rows, &grabbed);
        assert_eq!(drawn[0].entry.path, PathBuf::from("/p/c.rs"));
    }
}
