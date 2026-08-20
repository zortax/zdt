//! The sessionizer: choosing a directory to work in.
//!
//! Modelled on `tmux-sessionizer`. A fuzzy list of projects, where a project is any directory
//! under one of a few configured places, and choosing one attaches to that directory's session —
//! making it if there is none.
//!
//! Two things it is not. It is not a list of *saved* sessions: a directory that has never been
//! opened is as good a candidate as one that was open a minute ago. And it is not a restriction:
//! the configured places are only what is offered without asking, and any path at all can be
//! typed in and opened with `<C-x>`.

use std::path::{Path, PathBuf};

use crate::picker::{Deed, Picker, Row, Source, Target, Typed};
use crate::session::host::SessionHost;
use crate::session::{Session, SessionKey};

/// Where a chosen session is shown.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Where {
    /// In the window the picker was opened from.
    Here,
    /// In a window of its own, unless another window already holds it.
    ///
    /// One window looks at a session at a time: a split's editor is registered against the split,
    /// and two subtrees over one workspace would each claim the same registration.
    NewWindow,
}

/// The glyph a session that is open already gets.
const OPEN: &str = "\u{f0219}";
/// And one that is only a directory on disk.
const CLOSED: &str = "\u{f0770}";

/// Opens the sessionizer.
///
/// The live sessions are known at once and the directories are read on a worker, so the list
/// appears with what is open in it and fills in behind. A picker that waited for a walk of
/// `~/Projects` before drawing anything would be a picker nobody could use as a switcher.
pub fn open(session: &Session, place: Where) {
    let Some(picker) = zgui::reactive::use_local_context::<Picker>() else {
        return;
    };
    let host = crate::session::host::use_host();
    let settings = crate::settings::use_settings();
    let (places, depth, hidden) = settings.with_untracked(|config| {
        (
            config
                .sessions
                .paths
                .iter()
                .map(|text| zdt_core::config::expand_home(text))
                .collect::<Vec<_>>(),
            config.sessions.depth,
            config.sessions.hidden,
        )
    });

    // What is open already, and where this session is, both known without touching the disk.
    let here = session.key().clone();
    picker.open(source(rows(&host, &[], &here, place), &host, place));

    if places.is_empty() {
        return;
    }

    let (host, picker) = (host.clone(), picker.clone());
    zdt_view::detached(async move {
        let scanned = zgui::task::blocking(move || scan(&places, depth, hidden)).await;
        // The picker may have been closed, or opened on something else, while this ran. Filling
        // in a list nobody is looking at would take somebody's other picker away.
        if picker
            .source()
            .is_some_and(|source| source.title() == TITLE)
        {
            picker.open(source(rows(&host, &scanned, &here, place), &host, place));
        }
    });
}

/// What the sessionizer calls itself.
const TITLE: &str = "Sessions";

/// The list, with what to do about text that matched nothing.
fn source(rows: Vec<Row>, host: &SessionHost, place: Where) -> Source {
    let host = host.clone();
    Source::Given {
        title: TITLE,
        rows,
        // Any directory at all, typed in full. The configured places are a convenience and never
        // the whole of what can be opened.
        typed: Some(Typed::new(move |query| {
            let path = zdt_core::config::expand_home(query);
            match SessionKey::of(&path) {
                Some(key) => {
                    show(&host, key, place);
                }
                None => {
                    if let Some(client) = host.any_client()
                        && let Some(session) =
                            client.showing_untracked().and_then(|id| host.session(id))
                    {
                        session
                            .workspace()
                            .complain(format!("{} is not a directory", path.display()));
                    }
                }
            }
        })),
    }
}

/// Every candidate, open ones first.
///
/// A session that is open is worth more than a directory that is not: it has the buffers and the
/// terminals somebody left there. Within each group the order is what the scan gave, which is
/// alphabetical.
fn rows(host: &SessionHost, scanned: &[PathBuf], here: &SessionKey, place: Where) -> Vec<Row> {
    let open = host.list_untracked();
    let mut rows: Vec<Row> = Vec::with_capacity(open.len() + scanned.len());
    let mut seen: Vec<PathBuf> = Vec::with_capacity(open.len());

    for listed in &open {
        let Some(path) = listed.key.path() else {
            continue;
        };
        seen.push(path.to_path_buf());
        let mark = if &listed.key == here { "current" } else { "" };
        let detail = match (listed.buffers, mark) {
            (_, "current") => "current".to_owned(),
            (1, _) => "1 buffer".to_owned(),
            (count, _) => format!("{count} buffers"),
        };
        rows.push(
            Row::plain(label(path), reveal(host, listed.key.clone(), place))
                .with_detail(detail)
                .with_glyph(OPEN, "zdt-git-added"),
        );
    }

    for path in scanned {
        if seen.iter().any(|held| held == path) {
            continue;
        }
        let Some(key) = SessionKey::of(path) else {
            continue;
        };
        rows.push(
            Row::plain(label(path), reveal(host, key, place))
                .with_detail(String::new())
                .with_glyph(CLOSED, "zui-color-muted-foreground"),
        );
    }

    rows
}

/// What a row does: put that directory's session on screen.
fn reveal(host: &SessionHost, key: SessionKey, place: Where) -> Target {
    let host = host.clone();
    Target::Run(Deed::new(move || {
        show(&host, key.clone(), place);
    }))
}

/// Shows `key`'s session where `place` says.
fn show(host: &SessionHost, key: SessionKey, place: Where) {
    match place {
        Where::Here => host.reveal(key, &[]),
        Where::NewWindow => host.reveal_in_new_window(key, &[]),
    };
}

/// How a directory is written in the list.
///
/// The last two components, with the home directory shortened. A column of absolute paths is a
/// column where every row starts with the same twenty characters, and the fuzzy matcher would
/// have to be typed past before it saw anything that told the rows apart.
fn label(path: &Path) -> String {
    let shortened = zdt_core::config::shorten_home(path);
    let parts: Vec<&str> = shortened
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    if parts.len() <= 2 {
        return shortened;
    }
    parts[parts.len() - 2..].join("/")
}

/// Every directory under each of `places`, no deeper than `depth`.
///
/// Blocking. Called on a worker.
fn scan(places: &[PathBuf], depth: usize, hidden: bool) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();
    for place in places {
        found.extend(zdt_core::search::files::directories_within(
            place, depth, hidden,
        ));
    }
    found.sort_unstable();
    found.dedup();
    found
}

#[cfg(test)]
mod tests {
    use super::label;
    use std::path::Path;

    #[test]
    fn a_short_path_is_written_whole() {
        assert_eq!(label(Path::new("/opt")), "/opt");
        assert_eq!(label(Path::new("/opt/work")), "/opt/work");
    }

    #[test]
    fn a_long_path_is_written_as_its_last_two_parts() {
        // A column of full paths is a column that all looks the same for twenty characters.
        assert_eq!(label(Path::new("/one/two/three/four")), "three/four");
    }

    #[test]
    fn a_path_in_the_home_directory_is_shortened() {
        assert_eq!(label(Path::new("/one/two")), "/one/two");
    }
}
