//! The command line: `:w`, `:%s/a/b/g`, `:42`.
//!
//! Parsing only. What a command *does* needs a workspace, a filesystem and a window. This crate
//! knows about none of them, so it answers a description of what was asked for. The application
//! carries it out.
//!
//! # What is here
//!
//! Vim's command line is enormous. Here is the part people type: ranges, the file and buffer
//! commands, substitution, `:set`, and a bare line number.
//!
//! Everything else, such as `:global`, ranges that are arithmetic on marks, and command
//! definitions, is answered as unknown. A command that does nearly what was asked is worse than
//! one that says it does not know how.

use ropey::Rope;

/// One end of a range.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Address {
    /// A line, counting from one as the command line does.
    Line(usize),
    /// The line the caret is on. Written `.`.
    Current,
    /// The last line. Written `$`.
    Last,
    /// A mark. Written `'a`.
    Mark(char),
    /// Forward to the next match. Written `/pattern/`.
    Forward(String),
    /// Backward to the previous match. Written `?pattern?`.
    Backward(String),
}

/// Which lines a command applies to.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Range {
    /// The first line, when one was given.
    pub from: Option<Address>,
    /// The last, when one was.
    pub to: Option<Address>,
}

impl Range {
    /// Whether anything was given.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.from.is_none() && self.to.is_none()
    }

    /// The lines this covers, counting from zero, against `rope` and a caret on `line`.
    ///
    /// A range with one end is that one line. A range with none is the caret's line, which is what
    /// every command that takes one defaults to.
    #[must_use]
    pub fn lines(
        &self,
        rope: &Rope,
        line: usize,
        resolve_mark: impl Fn(char) -> Option<usize>,
    ) -> std::ops::RangeInclusive<usize> {
        let last = last_line(rope);
        let resolve = |address: &Address| -> usize {
            match address {
                Address::Line(number) => number.saturating_sub(1).min(last),
                Address::Current => line.min(last),
                Address::Last => last,
                Address::Mark(name) => resolve_mark(*name).unwrap_or(line).min(last),
                Address::Forward(pattern) => find(rope, pattern, line, true).unwrap_or(line),
                Address::Backward(pattern) => find(rope, pattern, line, false).unwrap_or(line),
            }
        };

        let from = self.from.as_ref().map_or(line.min(last), &resolve);
        let to = self.to.as_ref().map_or(from, &resolve);
        if from <= to { from..=to } else { to..=from }
    }
}

/// The last line `$` means.
///
/// A text ending in a break has an empty line after it that the caret can sit on. `G` goes to the
/// last line with something on it. `$` means the same line, for the same reason: `:%s` over a file
/// should not be a range with an empty line on the end.
fn last_line(rope: &Rope) -> usize {
    let last = rope.len_lines().saturating_sub(1);
    if last > 0 && rope.line(last).len_bytes() == 0 {
        last - 1
    } else {
        last
    }
}

/// The first line at or after `from` holding `pattern`, or before it when looking backward.
fn find(rope: &Rope, pattern: &str, from: usize, forward: bool) -> Option<usize> {
    let total = rope.len_lines();
    if total == 0 {
        return None;
    }
    // Wrapping, as vim's does: a search from the last line finds the first match from the top.
    (1..=total)
        .map(|step| {
            if forward {
                (from + step) % total
            } else {
                (from + total - (step % total)) % total
            }
        })
        .find(|line| rope.line(*line).to_string().contains(pattern))
}

/// What a command line asked for.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Command {
    /// Put the caret on a line. A bare `:42`.
    Goto(usize),
    /// Write the file, to `path` when one was given.
    Write {
        /// Where, when somewhere else was asked for.
        path: Option<String>,
        /// Whether to write whatever the buffer says about being read lossily.
        force: bool,
        /// Whether to close afterwards.
        then_quit: bool,
        /// Whether it applies to every buffer. To this one otherwise.
        all: bool,
    },
    /// Close the window, or the editor when it is the last.
    Quit {
        /// Whether to go without writing.
        force: bool,
        /// Whether every window.
        all: bool,
    },
    /// Open a file.
    Edit {
        /// Which, or nothing to read this one again.
        path: Option<String>,
        /// Whether to throw away unsaved changes.
        force: bool,
    },
    /// Close a buffer.
    BufferDelete {
        /// Whether to go without writing.
        force: bool,
    },
    /// Show another buffer: `:bn`, `:bp`, `:b3`.
    Buffer(BufferTarget),
    /// Divide the window.
    Split {
        /// Whether the split is side by side. One above the other otherwise.
        vertical: bool,
        /// What to open in it.
        path: Option<String>,
    },
    /// Replace text on the lines a range covers.
    Substitute {
        /// Which lines.
        range: Range,
        /// What to look for.
        pattern: String,
        /// What to put there.
        replacement: String,
        /// Whether every match on a line changes. Only the first otherwise.
        all: bool,
        /// Whether to ignore case whatever the pattern looks like.
        ignore_case: bool,
    },
    /// Change a setting: `:set number`, `:set tabstop=4`, `:set nowrap`.
    Set {
        /// Which one.
        name: String,
        /// What to, when a value was given. `None` with `off` false means "turn it on".
        value: Option<String>,
        /// Whether the name was written with `no` in front of it.
        off: bool,
        /// Whether the name was written with `!` after it.
        toggle: bool,
    },
    /// Put the search highlight away. Written `:noh`.
    NoHighlight,
    /// Run something in a terminal. Written `:!make`.
    Shell(String),
    /// Something this does not know.
    Unknown(String),
}

/// Which buffer `:b` was asking for.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum BufferTarget {
    /// The next one.
    Next,
    /// The one before.
    Previous,
    /// The first.
    First,
    /// The last.
    Last,
    /// The one whose name holds this.
    Named(String),
}

/// Reads one command line.
///
/// The leading `:` may be there or not; both are what a caller has.
#[must_use]
pub fn parse(line: &str) -> Option<Command> {
    let line = line.trim_start().trim_start_matches(':');
    if line.trim().is_empty() {
        return None;
    }

    // `:!make` is a shell line and nothing else, so it is read before anything looks for a range.
    if let Some(rest) = line.strip_prefix('!') {
        return Some(Command::Shell(rest.trim().to_owned()));
    }

    let (range, rest) = read_range(line);
    let rest = rest.trim_start();

    // A range with nothing after it is a jump: `:42`, `:$`, `:'a`.
    if rest.is_empty() {
        return match &range.to.or(range.from) {
            Some(Address::Line(number)) => Some(Command::Goto(*number)),
            Some(Address::Last) => Some(Command::Goto(usize::MAX)),
            Some(_) => None,
            None => None,
        };
    }

    // The name, the `!` that may follow it, and the rest of the line.
    let name_end = rest
        .find(|character: char| !character.is_ascii_alphabetic())
        .unwrap_or(rest.len());
    let (name, tail) = rest.split_at(name_end);
    let (force, tail) = match tail.strip_prefix('!') {
        Some(tail) => (true, tail),
        None => (false, tail),
    };
    let argument = tail.trim();
    let path = (!argument.is_empty()).then(|| argument.to_owned());

    Some(match name {
        "w" | "write" => Command::Write {
            path,
            force,
            then_quit: false,
            all: false,
        },
        "wa" | "wall" => Command::Write {
            path: None,
            force,
            then_quit: false,
            all: true,
        },
        "wq" | "x" | "xit" => Command::Write {
            path,
            force,
            then_quit: true,
            all: false,
        },
        "wqa" | "xa" | "xall" => Command::Write {
            path: None,
            force,
            then_quit: true,
            all: true,
        },
        "q" | "quit" => Command::Quit { force, all: false },
        "qa" | "qall" | "quitall" => Command::Quit { force, all: true },
        "e" | "edit" => Command::Edit { path, force },
        "bd" | "bdelete" => Command::BufferDelete { force },
        "bn" | "bnext" => Command::Buffer(BufferTarget::Next),
        "bp" | "bprev" | "bprevious" | "bN" => Command::Buffer(BufferTarget::Previous),
        "bf" | "bfirst" => Command::Buffer(BufferTarget::First),
        "bl" | "blast" => Command::Buffer(BufferTarget::Last),
        "b" | "buffer" => match path {
            Some(named) => Command::Buffer(BufferTarget::Named(named)),
            None => Command::Buffer(BufferTarget::Next),
        },
        "sp" | "split" | "new" => Command::Split {
            vertical: false,
            path,
        },
        "vs" | "vsp" | "vsplit" | "vnew" => Command::Split {
            vertical: true,
            path,
        },
        "s" | "substitute" => substitute(range, tail)?,
        "set" | "se" => setting(argument)?,
        "noh" | "nohl" | "nohlsearch" => Command::NoHighlight,
        other => Command::Unknown(other.to_owned()),
    })
}

/// Reads the range off the front of a line, answering it and whatever is left.
fn read_range(line: &str) -> (Range, &str) {
    // `%` is the whole file, which is the one range that is a single character.
    if let Some(rest) = line.strip_prefix('%') {
        return (
            Range {
                from: Some(Address::Line(1)),
                to: Some(Address::Last),
            },
            rest,
        );
    }

    let (from, rest) = read_address(line);
    let Some(rest) = rest.strip_prefix(',') else {
        return (
            Range {
                from: from.clone(),
                to: from,
            },
            rest,
        );
    };
    let (to, rest) = read_address(rest);
    (Range { from, to }, rest)
}

/// Reads one address off the front, answering it and whatever is left.
fn read_address(line: &str) -> (Option<Address>, &str) {
    let mut characters = line.char_indices();
    let Some((_, first)) = characters.next() else {
        return (None, line);
    };

    match first {
        '.' => (Some(Address::Current), &line[1..]),
        '$' => (Some(Address::Last), &line[1..]),
        '\'' => match line[1..].chars().next() {
            Some(name) => (Some(Address::Mark(name)), &line[1 + name.len_utf8()..]),
            None => (None, line),
        },
        '/' | '?' => {
            let closing = line[1..].find(first).map(|at| at + 1);
            let (pattern, rest) = match closing {
                Some(at) => (&line[1..at], &line[at + 1..]),
                None => (&line[1..], ""),
            };
            let pattern = pattern.to_owned();
            let address = if first == '/' {
                Address::Forward(pattern)
            } else {
                Address::Backward(pattern)
            };
            (Some(address), rest)
        }
        digit if digit.is_ascii_digit() => {
            let end = line
                .find(|character: char| !character.is_ascii_digit())
                .unwrap_or(line.len());
            let number = line[..end].parse().ok();
            (number.map(Address::Line), &line[end..])
        }
        _ => (None, line),
    }
}

/// Reads `s/pattern/replacement/flags`.
///
/// Any character may be the separator, as in vim: `s#a#b#` is a substitution over paths without a
/// backslash in sight.
fn substitute(range: Range, tail: &str) -> Option<Command> {
    let mut characters = tail.chars();
    let separator = characters.next()?;
    if separator.is_alphanumeric() {
        return None;
    }

    let rest = &tail[separator.len_utf8()..];
    let mut parts = split_unescaped(rest, separator);
    let pattern = parts.next().unwrap_or_default();
    let replacement = parts.next().unwrap_or_default();
    let flags = parts.next().unwrap_or_default();

    Some(Command::Substitute {
        range,
        pattern,
        replacement,
        all: flags.contains('g'),
        ignore_case: flags.contains('i'),
    })
}

/// Splits on `separator`, keeping one that was written `\/`.
fn split_unescaped(text: &str, separator: char) -> impl Iterator<Item = String> {
    let mut parts = Vec::new();
    let mut held = String::new();
    let mut escaped = false;

    for character in text.chars() {
        if escaped {
            // A backslash before the separator means the separator; before anything else it is
            // itself, because a pattern is full of them.
            if character != separator {
                held.push('\\');
            }
            held.push(character);
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character == separator {
            parts.push(std::mem::take(&mut held));
        } else {
            held.push(character);
        }
    }
    if escaped {
        held.push('\\');
    }
    parts.push(held);
    parts.into_iter()
}

/// Reads `set name`, `set noname`, `set name!`, `set name=value`.
fn setting(argument: &str) -> Option<Command> {
    let argument = argument.trim();
    if argument.is_empty() {
        return None;
    }

    let (name, value) = match argument.split_once('=') {
        Some((name, value)) => (name.trim(), Some(value.trim().to_owned())),
        None => (argument, None),
    };

    let (name, toggle) = match name.strip_suffix('!') {
        Some(name) => (name, true),
        None => (name, false),
    };
    // `nonumber` turns `number` off. Only when the rest is a name: `no` on its own names a
    // setting called `no`.
    let (name, off) = match name.strip_prefix("no") {
        Some(rest) if !rest.is_empty() && value.is_none() => (rest, true),
        _ => (name, false),
    };

    Some(Command::Set {
        name: name.to_owned(),
        value,
        off,
        toggle,
    })
}

#[cfg(test)]
mod tests;
