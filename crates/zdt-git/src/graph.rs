//! Laying a history out as lines and dots.
//!
//! A commit graph drawn as a picture is a scheduling problem: each commit gets a *lane*, and the
//! lines between rows say which commit came from which. This works it out from nothing but the
//! commits and their parents, so it is pure — no repository, no I/O — and can be asserted against
//! a history written out by hand.
//!
//! # The algorithm
//!
//! Walk newest to oldest, keeping a list of lanes. Each lane holds the commit it is *waiting for*.
//!
//! For each commit:
//!
//! 1. Find the lanes waiting for it. The leftmost is where its dot goes; every other one merges
//!    into that lane and is freed.
//! 2. If no lane was waiting — which happens for a branch tip — take the leftmost free lane.
//! 3. Put the commit's first parent in its own lane, and every other parent in a fresh lane
//!    beside it. That is what makes a merge fan out to the right and rejoin later.
//!
//! Newest-to-oldest matters. Drawn the other way round a branch's lane would not exist until the
//! commit that created it, and the line joining them would have nowhere to start.

use rustc_hash::FxHashMap;

use crate::log::Commit;

/// A line between one row and the next.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Edge {
    /// Which lane it leaves this row in.
    pub from: usize,
    /// Which lane it arrives in on the row below.
    pub to: usize,
}

/// One commit, placed.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Row {
    /// Which commit.
    pub id: String,
    /// Which lane its dot is in.
    pub lane: usize,
    /// The lines running from this row to the next.
    pub edges: Vec<Edge>,
    /// How many lanes are in use here, which is how wide the drawing has to be.
    pub width: usize,
}

/// Lays `commits` out, newest first.
///
/// The commits must be in the order they are drawn in — which is the order [`crate::log::log`]
/// returns them.
#[must_use]
pub fn lay_out(commits: &[Commit]) -> Vec<Row> {
    // What each lane is waiting for. `None` is a free lane.
    let mut lanes: Vec<Option<String>> = Vec::new();
    let mut rows = Vec::with_capacity(commits.len());

    for commit in commits {
        // Every lane waiting for this commit. The leftmost is where its dot goes; the others are
        // branches rejoining, and their lines bend into it.
        let waiting: Vec<usize> = lanes
            .iter()
            .enumerate()
            .filter(|(_, held)| held.as_deref() == Some(commit.id.as_str()))
            .map(|(at, _)| at)
            .collect();

        let lane = match waiting.first() {
            Some(first) => *first,
            // Nothing is waiting for it: a branch tip, or the newest commit. It takes the leftmost
            // free lane rather than a new one, so a history with one branch stays one column wide
            // however many branches have come and gone above it.
            None => free_lane(&mut lanes),
        };
        // The ones rejoining are freed; their lines are drawn below.
        for other in waiting.iter().skip(1) {
            lanes[*other] = None;
        }
        lanes[lane] = None;

        // Where each parent goes. The first stays in this commit's own lane, so a straight history
        // is a straight line; the rest fan out to the right.
        let mut edges: Vec<Edge> = Vec::new();
        for (at, parent) in commit.parents.iter().enumerate() {
            let target = if at == 0 {
                lanes[lane] = Some(parent.clone());
                lane
            } else {
                match lanes
                    .iter()
                    .position(|held| held.as_deref() == Some(parent.as_str()))
                {
                    // That parent is already expected somewhere: the merge rejoins an existing
                    // line rather than opening another one beside it.
                    Some(existing) => existing,
                    None => {
                        let fresh = free_lane(&mut lanes);
                        lanes[fresh] = Some(parent.clone());
                        fresh
                    }
                }
            };
            edges.push(Edge {
                from: lane,
                to: target,
            });
        }

        // Everything else that was passing through carries straight on.
        for (at, held) in lanes.iter().enumerate() {
            if held.is_some() && !edges.iter().any(|edge| edge.to == at) {
                edges.push(Edge { from: at, to: at });
            }
        }
        // The branches that rejoined here, drawn arriving from where they were.
        for other in waiting.iter().skip(1) {
            edges.push(Edge {
                from: *other,
                to: lane,
            });
        }

        edges.sort_by_key(|edge| (edge.from, edge.to));
        edges.dedup();

        rows.push(Row {
            id: commit.id.clone(),
            lane,
            edges,
            width: lanes
                .iter()
                .filter(|held| held.is_some())
                .count()
                .max(lane + 1),
        });

        // Lanes that have fallen off the right-hand end are dropped, so the drawing narrows again
        // when a branch is done.
        while lanes.last().is_some_and(Option::is_none) {
            lanes.pop();
        }
    }

    rows
}

/// The leftmost free lane, making one if every lane is busy.
fn free_lane(lanes: &mut Vec<Option<String>>) -> usize {
    match lanes.iter().position(Option::is_none) {
        Some(at) => at,
        None => {
            lanes.push(None);
            lanes.len() - 1
        }
    }
}

/// How wide the whole drawing is.
#[must_use]
pub fn width(rows: &[Row]) -> usize {
    rows.iter().map(|row| row.width).max().unwrap_or(0)
}

/// Which colour a lane is drawn in, as an index into the theme's set.
///
/// Eight of them, cycling. A colour per lane forever would be a colour nobody could tell from the
/// one two lanes over; eight is enough that two lines crossing are different colours and few
/// enough that each stays distinct.
#[must_use]
pub const fn lane_tint(lane: usize) -> usize {
    lane % 8
}

/// A commit map, for turning a history into rows and back.
#[must_use]
pub fn by_id(commits: &[Commit]) -> FxHashMap<String, usize> {
    commits
        .iter()
        .enumerate()
        .map(|(at, commit)| (commit.id.clone(), at))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{lay_out, width};
    use crate::log::Commit;

    /// A commit with the given id and parents, and nothing else that matters here.
    fn commit(id: &str, parents: &[&str]) -> Commit {
        Commit {
            id: id.to_owned(),
            short: id.to_owned(),
            summary: id.to_owned(),
            body: String::new(),
            author: "Test".to_owned(),
            email: "test@example.com".to_owned(),
            when: 0,
            parents: parents.iter().map(|id| (*id).to_owned()).collect(),
        }
    }

    #[test]
    fn a_straight_history_is_one_column() {
        // The common case by a very long way, and the one where a graph that widened would be
        // pure noise beside the text.
        let history = [commit("c", &["b"]), commit("b", &["a"]), commit("a", &[])];
        let rows = lay_out(&history);
        assert!(rows.iter().all(|row| row.lane == 0), "{rows:#?}");
        assert_eq!(width(&rows), 1);
    }

    #[test]
    fn every_row_carries_its_own_commit() {
        let history = [commit("b", &["a"]), commit("a", &[])];
        let rows = lay_out(&history);
        assert_eq!(
            rows.iter().map(|row| row.id.as_str()).collect::<Vec<_>>(),
            ["b", "a"]
        );
    }

    #[test]
    fn a_merge_fans_out_and_rejoins() {
        //   m      merge of main and side
        //   |\
        //   | s    the side branch
        //   |/
        //   a      where they parted
        let history = [
            commit("m", &["n", "s"]),
            commit("n", &["a"]),
            commit("s", &["a"]),
            commit("a", &[]),
        ];
        let rows = lay_out(&history);

        assert_eq!(rows[0].lane, 0, "the merge is on the trunk");
        assert_eq!(rows[0].edges.len(), 2, "one line to each parent");
        assert!(
            rows[0].edges.iter().any(|edge| edge.to != 0),
            "one of them goes to a lane of its own: {:#?}",
            rows[0].edges
        );
        assert!(width(&rows) >= 2, "the drawing widened for the branch");

        // And by the time the two histories meet again, everything is back in one lane.
        let last = rows.last().expect("there is a last row");
        assert_eq!(last.id, "a");
        assert_eq!(last.width, 1, "the drawing narrowed again: {rows:#?}");
    }

    #[test]
    fn two_tips_get_two_lanes() {
        // Two branches with no merge between them, which is what a repository looks like while
        // somebody has work on both.
        let history = [commit("x", &["a"]), commit("y", &["a"]), commit("a", &[])];
        let rows = lay_out(&history);
        assert_ne!(rows[0].lane, rows[1].lane, "the tips are side by side");
        assert_eq!(
            rows[2].lane, rows[0].lane,
            "and the shared parent is on the first"
        );
    }

    #[test]
    fn a_lane_is_reused_once_its_branch_has_ended() {
        // Otherwise a repository with a hundred merged branches would be a hundred columns wide,
        // ninety-nine of them empty.
        let history = [
            commit("d", &["c"]),
            commit("c", &["b"]),
            commit("b", &["a"]),
            commit("a", &[]),
        ];
        assert_eq!(width(&lay_out(&history)), 1);
    }

    #[test]
    fn nothing_at_all_lays_out_as_nothing() {
        assert!(lay_out(&[]).is_empty());
        assert_eq!(width(&[]), 0);
    }

    #[test]
    fn the_first_commit_ends_its_lane() {
        let history = [commit("a", &[])];
        let rows = lay_out(&history);
        assert!(
            rows[0].edges.is_empty(),
            "a commit with no parents has nothing below it: {:#?}",
            rows[0].edges
        );
    }
}
