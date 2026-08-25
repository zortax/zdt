//! What was asked for on the command line.
//!
//! Hand-written, and deliberately small. The whole vocabulary is a directory, some files, and
//! four flags — and the parsing runs before anything else, in a process that may turn out to be a
//! client that says one sentence and exits.

use std::path::PathBuf;

use crate::session::SessionKey;

/// What this invocation is for.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Launch {
    /// Work in a directory.
    Open(Open),
    /// Say what sessions are open, and exit.
    List,
    /// Take a directory's session away, and exit.
    Kill(PathBuf),
    /// Speak to the agent daemon, and exit.
    Agent(AgentVerb),
    /// Say how to use it, and exit.
    Help,
    /// Say which zdt this is, and exit.
    Version,
}

/// What to ask the agent daemon.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AgentVerb {
    /// Print every thread.
    List,
    /// Say whether a daemon runs, and which.
    Status,
    /// Ask the daemon to stop.
    Stop,
}

/// A directory to work in, and what to open in it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Open {
    /// Which directory's session.
    pub key: SessionKey,
    /// What to open in it.
    pub files: Vec<PathBuf>,
    /// Whether to run as an editor of this process's own, talking to nobody.
    pub standalone: bool,
    /// Whether it should get a window of its own.
    pub new_window: bool,
}

/// What to print for `--help`.
pub const USAGE: &str = "\
zdt — a modal editor

    zdt [OPTIONS] [DIRECTORY] [FILE ...]

A directory names the session to work in. With none, the session is the working directory, so
`zdt` is always in one. Files are opened into whichever session that is.

A zdt already working in that directory takes it over: this one hands the directory across and
exits, and the running editor opens or focuses a window for it.

OPTIONS
    -n, --new-window   Open the session in a window of its own
        --standalone   Do not hand over to a running zdt, and do not take handovers
        --list         List the sessions a running zdt has open
        --kill <DIR>   Take a running zdt's session for <DIR> away
    -h, --help         Print this
    -V, --version      Print the version

AGENT
    zdt agent list     List every agent thread
    zdt agent status   Say whether the agent daemon runs
    zdt agent stop     Stop the agent daemon; running turns are interrupted
";

/// Reads the command line.
///
/// A directory that will not canonicalise falls back to the working directory, and then to `.`:
/// there is nowhere better to start from, and refusing to start at all is worse.
#[must_use]
pub fn parse() -> Launch {
    parse_from(std::env::args_os().skip(1).map(PathBuf::from))
}

/// The same, over arguments given rather than taken from the environment.
#[must_use]
pub fn parse_from(arguments: impl Iterator<Item = PathBuf>) -> Launch {
    let mut directory: Option<PathBuf> = None;
    let mut files: Vec<PathBuf> = Vec::new();
    let mut standalone = false;
    let mut new_window = false;
    let mut arguments = arguments.peekable();

    let mut first = true;
    while let Some(argument) = arguments.next() {
        // The agent verbs stand first: `zdt agent list` speaks to the daemon and exits.
        if std::mem::take(&mut first) && argument.to_string_lossy() == "agent" {
            let verb = arguments
                .next()
                .map(|verb| verb.to_string_lossy().into_owned())
                .unwrap_or_default();
            return Launch::Agent(match verb.as_str() {
                "status" => AgentVerb::Status,
                "stop" => AgentVerb::Stop,
                _ => AgentVerb::List,
            });
        }
        match argument.to_string_lossy().as_ref() {
            "-h" | "--help" => return Launch::Help,
            "-V" | "--version" => return Launch::Version,
            "--list" => return Launch::List,
            "--kill" => {
                return Launch::Kill(arguments.next().unwrap_or_else(|| PathBuf::from(".")));
            }
            "--standalone" => standalone = true,
            "-n" | "--new-window" => new_window = true,
            // Everything else is a path. A directory names the session; the rest are files.
            _ => {
                if argument.is_dir() {
                    directory.get_or_insert(argument);
                } else {
                    files.push(argument);
                }
            }
        }
    }

    let real = |path: PathBuf| std::fs::canonicalize(&path).unwrap_or(path);
    let files: Vec<PathBuf> = files.into_iter().map(real).collect();
    let here = directory
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));

    let key = SessionKey::of(&here)
        .or_else(|| {
            std::env::current_dir()
                .ok()
                .and_then(|here| SessionKey::of(&here))
        })
        .unwrap_or_else(|| SessionKey::Local(real(here)));

    Launch::Open(Open {
        key,
        files,
        standalone,
        new_window,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(arguments: &[&str]) -> Launch {
        parse_from(arguments.iter().map(PathBuf::from))
    }

    fn open(arguments: &[&str]) -> Open {
        match parse(arguments) {
            Launch::Open(open) => open,
            other => panic!("expected an open, got {other:?}"),
        }
    }

    #[test]
    fn nothing_at_all_is_the_working_directory() {
        // Which is what makes `zdt` in a directory that directory's session, and what makes the
        // editor always in one.
        let here = std::env::current_dir().expect("there is one");
        assert_eq!(
            open(&[]).key,
            SessionKey::of(&here).expect("it is a directory")
        );
    }

    #[test]
    fn a_directory_names_the_session() {
        let here = env!("CARGO_MANIFEST_DIR");
        assert_eq!(
            open(&[here]).key,
            SessionKey::of(std::path::Path::new(here)).expect("it is a directory"),
        );
    }

    #[test]
    fn files_are_opened_into_whichever_session_it_is() {
        let here = env!("CARGO_MANIFEST_DIR");
        let file = format!("{here}/Cargo.toml");
        let asked = open(&[here, &file]);
        assert_eq!(asked.files.len(), 1);
        assert!(asked.files[0].ends_with("Cargo.toml"));
    }

    #[test]
    fn the_first_directory_wins_and_the_rest_are_files() {
        let here = env!("CARGO_MANIFEST_DIR");
        let src = format!("{here}/src");
        let asked = open(&[here, &src]);
        assert_eq!(
            asked.key,
            SessionKey::of(std::path::Path::new(here)).unwrap()
        );
        // The second directory is not a file either; it is simply not the session.
        assert!(asked.files.is_empty());
    }

    #[test]
    fn the_flags_are_read() {
        assert!(open(&["--standalone"]).standalone);
        assert!(open(&["-n"]).new_window);
        assert!(open(&["--new-window"]).new_window);
        assert_eq!(parse(&["--help"]), Launch::Help);
        assert_eq!(parse(&["-h"]), Launch::Help);
        assert_eq!(parse(&["-V"]), Launch::Version);
        assert_eq!(parse(&["--list"]), Launch::List);
    }

    #[test]
    fn kill_takes_the_directory_after_it() {
        assert_eq!(
            parse(&["--kill", "/home/someone/work"]),
            Launch::Kill(PathBuf::from("/home/someone/work")),
        );
    }

    #[test]
    fn kill_with_nothing_after_it_means_here() {
        assert_eq!(parse(&["--kill"]), Launch::Kill(PathBuf::from(".")));
    }

    #[test]
    fn a_flag_after_a_path_is_still_a_flag() {
        let here = env!("CARGO_MANIFEST_DIR");
        assert!(open(&[here, "--standalone"]).standalone);
    }

    #[test]
    fn a_file_that_is_not_there_is_still_a_file_to_open() {
        // Opening a name that does not exist yet is how a new file is made.
        let asked = open(&["not-here-yet.txt"]);
        assert_eq!(asked.files.len(), 1);
    }
}
