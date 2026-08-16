//! Looking inside the files.
//!
//! `grep-searcher` over the same walk the file picker uses, in this process. There is no `rg` to
//! find on the path, and no process to spawn per keystroke.
//!
//! A search is cancellable and reports as it goes. What matters about a grep over a large
//! repository is how long until the first hit is on the screen. Batches go back through a callback
//! the caller gives. On a worker, that callback posts to the interface thread.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use grep_regex::RegexMatcherBuilder;
use grep_searcher::{BinaryDetection, SearcherBuilder, Sink, SinkMatch};

use crate::search::files::Walk;

/// One line that matched.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Hit {
    /// Which file, relative to the project root.
    pub path: String,
    /// Which line, counting from one, the way an editor does.
    pub line: u64,
    /// Where in the line the match began, in bytes.
    pub column: usize,
    /// How long the match was, in bytes.
    pub length: usize,
    /// The line itself, with the ends trimmed.
    pub text: String,
}

/// What a search should look for and where.
#[derive(Clone, Debug)]
pub struct Query {
    /// What to look for.
    pub pattern: String,
    /// Whether `pattern` is a regular expression. Literal text otherwise.
    pub regex: bool,
    /// Whether a pattern with no capitals matches without regard to case.
    pub smart_case: bool,
    /// Which files to look in.
    pub walk: Walk,
    /// How many hits to stop at.
    pub limit: usize,
}

impl Default for Query {
    fn default() -> Self {
        Self {
            pattern: String::new(),
            regex: false,
            smart_case: true,
            walk: Walk::default(),
            limit: 10_000,
        }
    }
}

/// A running search, and the way to stop one.
///
/// Dropping this does not stop the search: a search that was cancelled by its own results going
/// away would be one nobody could hand off. Call [`Cancel::stop`].
#[derive(Clone, Default)]
pub struct Cancel {
    stopped: Arc<AtomicBool>,
}

impl Cancel {
    /// A token nothing has stopped.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Stops the search this belongs to, at the next file.
    pub fn stop(&self) {
        self.stopped.store(true, Ordering::Relaxed);
    }

    /// Whether it has been stopped.
    #[must_use]
    pub fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::Relaxed)
    }
}

/// What went wrong.
#[derive(Debug, thiserror::Error)]
pub enum GrepError {
    /// The pattern is not a regular expression.
    #[error("{0}")]
    Pattern(#[from] grep_regex::Error),
}

/// Searches every file under `root` for `query`, handing batches of hits to `report`.
///
/// `report` is called on the walk's own threads, so it must be cheap and sound to call from
/// several at once. Sending down a channel is the intended shape.
///
/// Blocking. Call it from a worker.
///
/// # Errors
///
/// If the pattern will not compile.
pub fn search(
    root: &Path,
    query: &Query,
    cancel: &Cancel,
    report: impl Fn(Vec<Hit>) + Send + Sync,
) -> Result<(), GrepError> {
    if query.pattern.is_empty() {
        return Ok(());
    }

    // Smart case: a pattern somebody typed in lower case is a pattern they did not mean to be
    // fussy about. One with a capital in it is one they did.
    let insensitive = query.smart_case && !query.pattern.chars().any(char::is_uppercase);
    let matcher = RegexMatcherBuilder::new()
        .case_insensitive(insensitive)
        .line_terminator(Some(b'\n'))
        .build(&pattern_of(query))?;

    let found = AtomicUsize::new(0);
    let mut builder = ignore::WalkBuilder::new(root);
    builder
        .hidden(!query.walk.hidden)
        .follow_links(query.walk.follow_links)
        .require_git(false)
        .git_ignore(!query.walk.ignored)
        .git_global(!query.walk.ignored)
        .git_exclude(!query.walk.ignored);

    let root = root.to_path_buf();
    builder.build_parallel().run(|| {
        let (matcher, cancel, report, found, root) = (
            matcher.clone(),
            cancel.clone(),
            &report,
            &found,
            root.clone(),
        );
        let mut searcher = SearcherBuilder::new()
            // A binary file has no lines to show, and searching one is time spent on an answer
            // nobody can read.
            .binary_detection(BinaryDetection::quit(0))
            .line_number(true)
            .build();

        Box::new(move |entry| {
            if cancel.is_stopped() || found.load(Ordering::Relaxed) >= query.limit {
                return ignore::WalkState::Quit;
            }
            let Ok(entry) = entry else {
                return ignore::WalkState::Continue;
            };
            if entry.file_type().is_none_or(|kind| kind.is_dir()) {
                return ignore::WalkState::Continue;
            }

            let path = entry.path();
            let relative = path
                .strip_prefix(&root)
                .unwrap_or(path)
                .to_string_lossy()
                .into_owned();
            let mut collected = Collect {
                path: relative,
                hits: Vec::new(),
                matcher: &matcher,
            };
            let _ = searcher.search_path(&matcher, path, &mut collected);

            if !collected.hits.is_empty() {
                found.fetch_add(collected.hits.len(), Ordering::Relaxed);
                report(collected.hits);
            }
            ignore::WalkState::Continue
        })
    });

    Ok(())
}

/// The regular expression a query comes to.
fn pattern_of(query: &Query) -> String {
    if query.regex {
        query.pattern.clone()
    } else {
        regex_syntax::escape(&query.pattern)
    }
}

/// Gathers one file's hits.
struct Collect<'a, M> {
    path: String,
    hits: Vec<Hit>,
    /// The same matcher the search ran with, asked again for *where* in the line it matched.
    ///
    /// The searcher reports which lines matched, not which characters; a preview that wants to
    /// underline the word has to ask a second time.
    matcher: &'a M,
}

impl<M: grep_matcher::Matcher> Sink for Collect<'_, M> {
    type Error = std::io::Error;

    fn matched(
        &mut self,
        _searcher: &grep_searcher::Searcher,
        matched: &SinkMatch<'_>,
    ) -> Result<bool, Self::Error> {
        let bytes = matched.bytes();
        let text = String::from_utf8_lossy(bytes);
        // A line with a thousand columns of minified JavaScript in it is not a line anybody reads
        // in a picker, and shipping it costs more than leaving it out.
        let trimmed: String = text
            .trim_end_matches(['\n', '\r'])
            .chars()
            .take(400)
            .collect();

        let (column, length) = self
            .matcher
            .find_at(bytes, 0)
            .ok()
            .flatten()
            .map_or((0, 0), |found| (found.start(), found.end() - found.start()));

        self.hits.push(Hit {
            path: self.path.clone(),
            line: matched.line_number().unwrap_or(0),
            column,
            length,
            text: trimmed,
        });
        Ok(true)
    }
}

/// The absolute path a hit stands for.
#[must_use]
pub fn absolute(root: &Path, hit: &Hit) -> PathBuf {
    root.join(&hit.path)
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    /// A project with something to find in it.
    struct Temp(PathBuf);

    impl Temp {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!("zdt-grep-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(root.join("src")).expect("a directory");
            std::fs::write(root.join("src/one.rs"), "fn alpha() {}\nfn beta() {}\n")
                .expect("a file");
            std::fs::write(root.join("src/two.rs"), "// ALPHA is loud\n").expect("a file");
            Self(root)
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Every hit a search reports, in path order.
    fn run(root: &Path, query: &Query) -> Vec<Hit> {
        let found = Mutex::new(Vec::new());
        search(root, query, &Cancel::new(), |batch| {
            found
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .extend(batch);
        })
        .expect("the pattern compiles");
        let mut found = found
            .into_inner()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        found.sort_by(|left, right| (&left.path, left.line).cmp(&(&right.path, right.line)));
        found
    }

    #[test]
    fn a_hit_says_where_in_the_line_it_matched() {
        // What a preview underlines. The searcher reports the line; the column is asked of the
        // matcher a second time, and a preview without it can only highlight the whole line.
        let temp = Temp::new("column");
        let found = run(
            &temp.0,
            &Query {
                pattern: "beta".to_owned(),
                ..Query::default()
            },
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].text, "fn beta() {}");
        assert_eq!(found[0].column, 3, "`beta` begins three bytes in");
        assert_eq!(found[0].length, 4);
    }

    #[test]
    fn a_lower_case_pattern_is_not_fussy_about_case() {
        let temp = Temp::new("smart");
        let found = run(
            &temp.0,
            &Query {
                pattern: "alpha".to_owned(),
                ..Query::default()
            },
        );
        assert_eq!(found.len(), 2, "it found the loud one too");
        assert_eq!(found[0].path, "src/one.rs");
        assert_eq!(found[0].line, 1);
        assert_eq!(found[0].text, "fn alpha() {}");
    }

    #[test]
    fn a_capital_makes_it_fussy() {
        let temp = Temp::new("case");
        let found = run(
            &temp.0,
            &Query {
                pattern: "ALPHA".to_owned(),
                ..Query::default()
            },
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].path, "src/two.rs");
    }

    #[test]
    fn text_is_taken_literally_unless_it_is_a_pattern() {
        let temp = Temp::new("literal");
        // `()` is a group in a regular expression and two characters in a name.
        let found = run(
            &temp.0,
            &Query {
                pattern: "beta()".to_owned(),
                ..Query::default()
            },
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].text, "fn beta() {}");
    }

    #[test]
    fn a_pattern_that_will_not_compile_says_so() {
        let temp = Temp::new("broken");
        let outcome = search(
            &temp.0,
            &Query {
                pattern: "(unclosed".to_owned(),
                regex: true,
                ..Query::default()
            },
            &Cancel::new(),
            |_| {},
        );
        assert!(outcome.is_err());
    }

    #[test]
    fn a_stopped_search_reports_nothing() {
        let temp = Temp::new("cancel");
        let cancel = Cancel::new();
        cancel.stop();
        let found = Mutex::new(Vec::new());
        search(
            &temp.0,
            &Query {
                pattern: "fn".to_owned(),
                ..Query::default()
            },
            &cancel,
            |batch| {
                found
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .extend(batch);
            },
        )
        .expect("the pattern compiles");
        assert!(
            found
                .into_inner()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_empty()
        );
    }

    #[test]
    fn an_empty_pattern_finds_nothing() {
        let temp = Temp::new("empty");
        assert!(run(&temp.0, &Query::default()).is_empty());
    }
}
