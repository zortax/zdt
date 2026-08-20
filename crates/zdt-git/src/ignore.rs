//! Adding to `.gitignore`.
//!
//! The one at the top of the working tree, and never one beside the file. A rule in the root is
//! the rule a person goes looking for, and it is the only place an anchored rule means what it
//! says.

use crate::repo::{Error, Repo};

/// What is being left out.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// One file.
    File,
    /// A directory and everything under it.
    Directory,
}

/// Adds `path` to the working tree's `.gitignore`.
///
/// `path` is named the way git names it, from the top of the working tree.
///
/// The rule is anchored, so `/target` and never `target`: a rule meant for one directory that
/// matches every directory with that name hides somebody's source file three levels down. A
/// directory gets a trailing slash, so the rule says what it is about. Glob characters in a name
/// are escaped, so a file called `a[1].txt` names itself and nothing else.
///
/// The file is made when there is none. A rule that is already there is left where it is.
///
/// # Errors
///
/// When `.gitignore` cannot be read or written.
pub fn add(repo: &Repo, path: &str, kind: Kind) -> Result<(), Error> {
    use std::io::Write;

    let rule = rule_for(path, kind);
    let file = repo.root().join(".gitignore");
    let blame = |error: std::io::Error| Error::Git(format!("{}: {error}", file.display()));

    let held = match std::fs::read_to_string(&file) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(blame(error)),
    };
    if held.lines().any(|line| line.trim() == rule) {
        return Ok(());
    }

    let mut writing = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(&file)
        .map_err(blame)?;
    // A file whose last line has no break would otherwise take the new rule onto the end of it,
    // and the two together name nothing.
    if !held.is_empty() && !held.ends_with('\n') {
        writing.write_all(b"\n").map_err(blame)?;
    }
    writeln!(writing, "{rule}").map_err(blame)?;
    Ok(())
}

/// The line that leaves `path` out.
fn rule_for(path: &str, kind: Kind) -> String {
    let escaped = escaped(path.trim_matches('/'));
    match kind {
        Kind::File => format!("/{escaped}"),
        Kind::Directory => format!("/{escaped}/"),
    }
}

/// `path` with the characters gitignore reads as a pattern made literal.
fn escaped(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for character in path.chars() {
        if matches!(character, '\\' | '*' | '?' | '[' | ']' | '!' | '#') {
            out.push('\\');
        }
        out.push(character);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{Kind, add};
    use crate::repo::testing::Temp;

    /// Whether git itself leaves `path` out.
    ///
    /// Run here rather than through `Temp::run`, because `check-ignore` answers "it is not left
    /// out" by failing, and a helper that treats a failure as a fault cannot ask the question.
    fn left_out(temp: &Temp, path: &str) -> bool {
        std::process::Command::new("git")
            .arg("-C")
            .arg(temp.root())
            .args(["check-ignore", "-q", path])
            .status()
            .expect("git runs")
            .success()
    }

    #[test]
    fn a_rule_leaves_out_what_it_names() {
        // Asserted against git itself, because what is being tested is whether git agrees.
        let temp = Temp::new("ignore-add");
        temp.commit("a.txt", "one\n", "first");
        std::fs::create_dir_all(temp.path("target")).expect("made");
        temp.write("target/out.o", "");

        add(&temp.repo(), "target", Kind::Directory).expect("it adds");

        assert!(left_out(&temp, "target/out.o"));
        assert!(!left_out(&temp, "a.txt"));
    }

    #[test]
    fn a_rule_is_anchored_at_the_top_of_the_tree() {
        // An unanchored `build` would take `src/build.rs`'s directory with it.
        let temp = Temp::new("ignore-anchored");
        temp.commit("a.txt", "one\n", "first");
        std::fs::create_dir_all(temp.path("src/build")).expect("made");
        std::fs::create_dir_all(temp.path("build")).expect("made");
        temp.write("src/build/keep.rs", "");
        temp.write("build/out.o", "");

        add(&temp.repo(), "build", Kind::Directory).expect("it adds");

        assert!(left_out(&temp, "build/out.o"));
        assert!(!left_out(&temp, "src/build/keep.rs"));
    }

    #[test]
    fn a_name_with_a_pattern_character_in_it_names_only_itself() {
        let temp = Temp::new("ignore-escaped");
        temp.commit("a.txt", "one\n", "first");
        temp.write("a[1].txt", "");
        temp.write("a1.txt", "");

        add(&temp.repo(), "a[1].txt", Kind::File).expect("it adds");

        assert!(left_out(&temp, "a[1].txt"));
        assert!(!left_out(&temp, "a1.txt"));
    }

    #[test]
    fn the_same_rule_twice_is_written_once() {
        let temp = Temp::new("ignore-twice");
        temp.commit("a.txt", "one\n", "first");

        let repo = temp.repo();
        add(&repo, "target", Kind::Directory).expect("it adds");
        add(&repo, "target", Kind::Directory).expect("it adds again");

        let text = std::fs::read_to_string(temp.path(".gitignore")).expect("it reads");
        assert_eq!(text.matches("/target/").count(), 1, "{text}");
    }

    #[test]
    fn a_file_whose_last_line_has_no_break_gets_one() {
        let temp = Temp::new("ignore-newline");
        temp.commit("a.txt", "one\n", "first");
        temp.write(".gitignore", "first");

        add(&temp.repo(), "second", Kind::File).expect("it adds");

        let text = std::fs::read_to_string(temp.path(".gitignore")).expect("it reads");
        assert_eq!(text, "first\n/second\n");
    }

    #[test]
    fn a_file_that_is_not_there_is_made() {
        let temp = Temp::new("ignore-made");
        temp.commit("a.txt", "one\n", "first");
        assert!(!temp.path(".gitignore").exists());

        add(&temp.repo(), "out.o", Kind::File).expect("it adds");

        assert_eq!(
            std::fs::read_to_string(temp.path(".gitignore")).expect("it reads"),
            "/out.o\n"
        );
    }
}
