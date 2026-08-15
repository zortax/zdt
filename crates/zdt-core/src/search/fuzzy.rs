//! Ranking a list against what somebody has typed.
//!
//! Two things, for two sizes of problem.
//!
//! [`rank`] is the small one: a few hundred buffers, commands or themes, matched and sorted in one
//! call. It allocates a matcher each time, which at that size is not worth avoiding.
//!
//! [`Ranker`] is the large one: a hundred thousand paths, matched on a pool of threads while the
//! interface stays responsive. It is a thin cover over `nucleo` — the value it adds is that the
//! caller never sees a matcher, a pattern object or a tick timeout, only "here are the candidates"
//! and "here is what somebody typed".
//!
//! Both rank the same way, so a picker that outgrows one can move to the other without the results
//! changing under its user.

use std::sync::Arc;

use nucleo::pattern::{CaseMatching, Normalization};
use nucleo::{Config, Nucleo, Utf32Str};

/// One candidate that matched, and how well.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Ranked {
    /// Where it was in the list handed in.
    pub index: usize,
    /// How well it matched. Larger is better; only the order is meaningful.
    pub score: u32,
    /// Which bytes of the candidate the pattern landed on, for drawing.
    pub matched: Vec<u32>,
}

/// Ranks `candidates` against `pattern`, best first.
///
/// An empty pattern matches everything, in the order handed in, which is what a picker shows
/// before anybody has typed. Blocking, and linear — for a list small enough that linear is fine.
#[must_use]
pub fn rank(candidates: &[String], pattern: &str, limit: usize) -> Vec<Ranked> {
    let mut matcher = nucleo::Matcher::new(Config::DEFAULT.match_paths());

    if pattern.is_empty() {
        return candidates
            .iter()
            .enumerate()
            .take(limit)
            .map(|(index, _)| Ranked {
                index,
                score: 0,
                matched: Vec::new(),
            })
            .collect();
    }

    let parsed =
        nucleo::pattern::Pattern::parse(pattern, CaseMatching::Smart, Normalization::Smart);

    let mut scored: Vec<Ranked> = Vec::new();
    let mut haystack = Vec::new();
    let mut matched = Vec::new();
    for (index, candidate) in candidates.iter().enumerate() {
        haystack.clear();
        matched.clear();
        let text = Utf32Str::new(candidate, &mut haystack);
        if let Some(score) = parsed.indices(text, &mut matcher, &mut matched) {
            scored.push(Ranked {
                index,
                score,
                matched: matched.clone(),
            });
        }
    }

    // Ties break on the earlier candidate, so that a list somebody has already read does not
    // reshuffle itself between two keystrokes that score the same.
    scored.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.index.cmp(&right.index))
    });
    scored.truncate(limit);
    scored
}

/// A ranking that runs on its own threads.
///
/// For the file picker, where the candidate list is the whole project and re-ranking it on the
/// interface thread would be a stutter on every keystroke.
///
/// The shape of its use is: build one, push every candidate, then on each keystroke call
/// [`Ranker::seek`] and thereafter [`Ranker::poll`] until it says it has stopped. `poll` is cheap
/// and does not block; it is meant to be called from a timer a few times a second.
pub struct Ranker {
    inner: Nucleo<String>,
    /// What was last asked for, so that asking for the same thing again costs nothing.
    pattern: String,
}

impl Ranker {
    /// A ranking with nothing in it yet.
    ///
    /// `wake` is called from a worker whenever there is something new to poll for. It must be
    /// cheap and safe to call from any thread — waking the interface is what it is for.
    #[must_use]
    pub fn new(wake: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            inner: Nucleo::new(Config::DEFAULT.match_paths(), Arc::new(wake), None, 1),
            pattern: String::new(),
        }
    }

    /// Puts `candidates` in, replacing whatever was there.
    pub fn fill(&mut self, candidates: Vec<String>) {
        self.inner.restart(true);
        let injector = self.inner.injector();
        for candidate in candidates {
            injector.push(candidate, |candidate, columns| {
                columns[0] = candidate.as_str().into();
            });
        }
        // The pattern has to be said again after a restart, or the new candidates are matched
        // against nothing and everything appears.
        let pattern = std::mem::take(&mut self.pattern);
        self.seek(&pattern);
    }

    /// Asks for `pattern`. Cheap, and idempotent.
    pub fn seek(&mut self, pattern: &str) {
        if self.pattern == pattern && !pattern.is_empty() {
            return;
        }
        self.inner.pattern.reparse(
            0,
            pattern,
            CaseMatching::Smart,
            Normalization::Smart,
            // Whether this pattern only adds to the last one: when it does, the matcher narrows
            // what it already has instead of starting over, which is what makes typing feel free.
            pattern.starts_with(&self.pattern),
        );
        self.pattern = pattern.to_owned();
    }

    /// Lets the matcher get on with it for a moment, and says whether it is still going.
    ///
    /// Answers whether anything changed, so a caller can leave the interface alone when nothing
    /// has.
    pub fn poll(&mut self) -> Progress {
        let status = self.inner.tick(10);
        Progress {
            changed: status.changed,
            running: status.running,
        }
    }

    /// The best `limit` matches, as (candidate, where the pattern landed).
    #[must_use]
    pub fn matches(&self, limit: usize) -> Vec<(String, Vec<u32>)> {
        let snapshot = self.inner.snapshot();
        let count = snapshot.matched_item_count().min(limit as u32);
        if count == 0 {
            return Vec::new();
        }

        let mut matcher = nucleo::Matcher::new(Config::DEFAULT.match_paths());
        let mut haystack = Vec::new();
        let mut landed = Vec::new();
        snapshot
            .matched_items(..count)
            .map(|item| {
                haystack.clear();
                landed.clear();
                let text = Utf32Str::new(item.data, &mut haystack);
                snapshot
                    .pattern()
                    .column_pattern(0)
                    .indices(text, &mut matcher, &mut landed);
                (item.data.clone(), landed.clone())
            })
            .collect()
    }

    /// How many matched, and how many there are altogether.
    #[must_use]
    pub fn counts(&self) -> (u32, u32) {
        let snapshot = self.inner.snapshot();
        (snapshot.matched_item_count(), snapshot.item_count())
    }
}

/// What a [`Ranker::poll`] found.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Progress {
    /// Whether the matches are different from last time.
    pub changed: bool,
    /// Whether the matcher is still working.
    pub running: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names() -> Vec<String> {
        [
            "src/ui/picker/mod.rs",
            "src/ui/picker/list.rs",
            "src/ui/tree.rs",
            "Cargo.toml",
            "README.md",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect()
    }

    #[test]
    fn an_empty_pattern_keeps_the_order_it_was_given() {
        let candidates = names();
        let ranked = rank(&candidates, "", 10);
        assert_eq!(ranked.len(), 5);
        assert_eq!(ranked[0].index, 0);
        assert_eq!(ranked[4].index, 4);
    }

    #[test]
    fn the_closest_match_comes_first() {
        let candidates = names();
        let ranked = rank(&candidates, "picker", 10);
        assert_eq!(ranked.len(), 2);
        for held in &ranked {
            assert!(candidates[held.index].contains("picker"));
        }
    }

    #[test]
    fn letters_scattered_through_a_path_still_match() {
        let candidates = names();
        let ranked = rank(&candidates, "utr", 10);
        assert!(
            ranked
                .iter()
                .any(|held| candidates[held.index] == "src/ui/tree.rs"),
            "u-i, t-ree, r-s"
        );
    }

    #[test]
    fn what_matched_is_reported_for_drawing() {
        let candidates = vec!["Cargo.toml".to_owned()];
        let ranked = rank(&candidates, "cargo", 10);
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].matched, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn a_pattern_nothing_matches_is_an_empty_list() {
        let candidates = names();
        assert!(rank(&candidates, "zzzzzz", 10).is_empty());
    }

    #[test]
    fn the_limit_is_kept() {
        let candidates = names();
        assert_eq!(rank(&candidates, "", 2).len(), 2);
        assert_eq!(rank(&candidates, "r", 1).len(), 1);
    }

    #[test]
    fn a_ranker_narrows_as_it_is_told_more() {
        let mut ranker = Ranker::new(|| {});
        ranker.fill(names());

        // Poll until it has settled: the matching happens on other threads.
        for _ in 0..100 {
            if !ranker.poll().running {
                break;
            }
        }
        assert_eq!(ranker.counts().1, 5, "every candidate is in");

        ranker.seek("picker");
        for _ in 0..100 {
            if !ranker.poll().running {
                break;
            }
        }
        let matched = ranker.matches(10);
        assert_eq!(matched.len(), 2);
        assert!(matched.iter().all(|(path, _)| path.contains("picker")));
        assert!(
            matched.iter().all(|(_, landed)| !landed.is_empty()),
            "and it says where the pattern landed"
        );
    }
}
