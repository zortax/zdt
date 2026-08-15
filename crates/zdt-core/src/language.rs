//! What a file is, from its name.
//!
//! Two answers come from one table: which grammar highlights it, and which glyph stands for it in
//! the tree and on its buffer tab. They are together because they are decided by the same thing
//! and would otherwise drift — a file type that highlights but has no icon looks like a bug, and
//! one with an icon that does not highlight looks like a worse one.
//!
//! Glyphs are from a Nerd Font, written as escapes so that this file is readable in an editor that
//! has not got one. The colour is a token name the style sheet answers for, so a devicon follows
//! the theme like everything else.

use std::path::Path;

/// What one kind of file is called, drawn as, and highlighted by.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FileType {
    /// The grammar's name in the editor's registry, when there is one.
    pub language: Option<&'static str>,
    /// The glyph that stands for it.
    pub glyph: &'static str,
    /// The custom property its glyph is drawn with, without the leading dashes.
    pub tint: &'static str,
}

/// What a file with no rule for it is.
pub const UNKNOWN: FileType = FileType {
    language: None,
    glyph: "\u{f15b}",
    tint: "zui-color-muted-foreground",
};

/// A directory, closed.
pub const DIRECTORY: FileType = FileType {
    language: None,
    glyph: "\u{f07b}",
    tint: "zui-color-primary",
};

/// A directory, open.
pub const DIRECTORY_OPEN: FileType = FileType {
    language: None,
    glyph: "\u{f07c}",
    tint: "zui-color-primary",
};

/// Declares the table, by extension.
macro_rules! types {
    ($($ext:literal => ($language:expr, $glyph:literal, $tint:literal),)*) => {
        /// Every extension this knows, with what it means.
        pub const BY_EXTENSION: &[(&str, FileType)] = &[$((
            $ext,
            FileType { language: $language, glyph: $glyph, tint: $tint },
        )),*];
    };
}

types! {
    "rs"       => (Some("rust"), "\u{e7a8}", "zdt-icon-rust"),
    "toml"     => (Some("toml"), "\u{e6b2}", "zdt-icon-config"),
    "md"       => (Some("markdown"), "\u{e73e}", "zdt-icon-doc"),
    "markdown" => (Some("markdown"), "\u{e73e}", "zdt-icon-doc"),
    "json"     => (Some("json"), "\u{e60b}", "zdt-icon-config"),
    "jsonc"    => (Some("json"), "\u{e60b}", "zdt-icon-config"),
    "yaml"     => (Some("yaml"), "\u{e615}", "zdt-icon-config"),
    "yml"      => (Some("yaml"), "\u{e615}", "zdt-icon-config"),
    "py"       => (Some("python"), "\u{e73c}", "zdt-icon-python"),
    "pyi"      => (Some("python"), "\u{e73c}", "zdt-icon-python"),
    "ts"       => (Some("typescript"), "\u{e628}", "zdt-icon-ts"),
    "tsx"      => (Some("tsx"), "\u{e7ba}", "zdt-icon-ts"),
    "js"       => (Some("javascript"), "\u{e781}", "zdt-icon-js"),
    "jsx"      => (Some("javascript"), "\u{e7ba}", "zdt-icon-js"),
    "mjs"      => (Some("javascript"), "\u{e781}", "zdt-icon-js"),
    "go"       => (Some("go"), "\u{e627}", "zdt-icon-go"),
    "c"        => (Some("c"), "\u{e61e}", "zdt-icon-c"),
    "h"        => (Some("c"), "\u{e61e}", "zdt-icon-c"),
    "cpp"      => (Some("cpp"), "\u{e61d}", "zdt-icon-c"),
    "cc"       => (Some("cpp"), "\u{e61d}", "zdt-icon-c"),
    "hpp"      => (Some("cpp"), "\u{e61d}", "zdt-icon-c"),
    "lua"      => (Some("lua"), "\u{e620}", "zdt-icon-lua"),
    "sh"       => (Some("bash"), "\u{f489}", "zdt-icon-shell"),
    "bash"     => (Some("bash"), "\u{f489}", "zdt-icon-shell"),
    "zsh"      => (Some("bash"), "\u{f489}", "zdt-icon-shell"),
    "fish"     => (Some("bash"), "\u{f489}", "zdt-icon-shell"),
    "html"     => (Some("html"), "\u{e736}", "zdt-icon-html"),
    "css"      => (Some("css"), "\u{e749}", "zdt-icon-css"),
    "scss"     => (Some("css"), "\u{e749}", "zdt-icon-css"),
    "txt"      => (None, "\u{f15c}", "zui-color-muted-foreground"),
    "lock"     => (None, "\u{f023}", "zui-color-muted-foreground"),
    "png"      => (None, "\u{f1c5}", "zdt-icon-image"),
    "jpg"      => (None, "\u{f1c5}", "zdt-icon-image"),
    "jpeg"     => (None, "\u{f1c5}", "zdt-icon-image"),
    "svg"      => (None, "\u{f1c5}", "zdt-icon-image"),
    "gif"      => (None, "\u{f1c5}", "zdt-icon-image"),
    "webp"     => (None, "\u{f1c5}", "zdt-icon-image"),
}

/// Whole names that decide a file's type on their own, whatever they end in.
const BY_NAME: &[(&str, FileType)] = &[
    (
        "Cargo.lock",
        FileType {
            language: Some("toml"),
            glyph: "\u{e7a8}",
            tint: "zui-color-muted-foreground",
        },
    ),
    (
        "Makefile",
        FileType {
            language: None,
            glyph: "\u{e673}",
            tint: "zdt-icon-config",
        },
    ),
    (
        "Dockerfile",
        FileType {
            language: None,
            glyph: "\u{f308}",
            tint: "zdt-icon-config",
        },
    ),
    (
        ".gitignore",
        FileType {
            language: None,
            glyph: "\u{e702}",
            tint: "zdt-icon-git",
        },
    ),
    (
        ".gitmodules",
        FileType {
            language: None,
            glyph: "\u{e702}",
            tint: "zdt-icon-git",
        },
    ),
    (
        "LICENSE",
        FileType {
            language: None,
            glyph: "\u{f718}",
            tint: "zui-color-muted-foreground",
        },
    ),
];

/// What a terminal buffer shows on the buffer line.
pub const TERMINAL: FileType = FileType {
    language: None,
    glyph: "\u{f489}",
    tint: "zui-color-muted-foreground",
};

/// What `path` is.
///
/// Whole names win over extensions, because `Cargo.lock` is a lock file before it is a `.lock`
/// file and the two want different glyphs.
#[must_use]
pub fn of(path: &Path) -> FileType {
    if let Some(name) = path.file_name().and_then(|name| name.to_str())
        && let Some((_, kind)) = BY_NAME.iter().find(|(held, _)| *held == name)
    {
        return *kind;
    }
    path.extension()
        .and_then(|extension| extension.to_str())
        .and_then(of_extension)
        .unwrap_or(UNKNOWN)
}

/// What a file ending in `extension` is, when anything.
#[must_use]
pub fn of_extension(extension: &str) -> Option<FileType> {
    let lowered = extension.to_ascii_lowercase();
    BY_EXTENSION
        .iter()
        .find(|(held, _)| *held == lowered)
        .map(|(_, kind)| *kind)
}

/// Which grammar highlights `path`, when one does.
#[must_use]
pub fn language_of(path: &Path) -> Option<&'static str> {
    of(path).language
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{BY_EXTENSION, UNKNOWN, language_of, of};

    #[test]
    fn a_rust_file_highlights_as_rust() {
        assert_eq!(language_of(Path::new("src/main.rs")), Some("rust"));
        assert_eq!(language_of(Path::new("/tmp/a/b/lib.rs")), Some("rust"));
    }

    #[test]
    fn a_whole_name_beats_its_extension() {
        // `Cargo.lock` is TOML with a lock file's glyph, not a `.lock` file with none.
        let lock = of(Path::new("Cargo.lock"));
        assert_eq!(lock.language, Some("toml"));
        assert_ne!(lock.glyph, UNKNOWN.glyph);
    }

    #[test]
    fn an_extension_is_matched_whatever_its_case() {
        assert_eq!(language_of(Path::new("README.MD")), Some("markdown"));
        assert_eq!(language_of(Path::new("Photo.PNG")), None);
    }

    #[test]
    fn something_unknown_is_still_a_file() {
        let kind = of(Path::new("notes.qqq"));
        assert_eq!(kind.language, None);
        assert_eq!(kind.glyph, UNKNOWN.glyph);
    }

    #[test]
    fn a_file_with_no_extension_at_all_is_unknown() {
        assert_eq!(of(Path::new("notes")).glyph, UNKNOWN.glyph);
        assert_eq!(language_of(Path::new("notes")), None);
    }

    #[test]
    fn every_glyph_is_one_character() {
        // A two-character glyph would push the text of every row it is on out of line with every
        // other, which is the whole reason the tree looks tidy.
        for (extension, kind) in BY_EXTENSION {
            assert_eq!(
                kind.glyph.chars().count(),
                1,
                "the glyph for .{extension} is not one character"
            );
        }
    }

    #[test]
    fn the_table_names_each_extension_once() {
        let mut seen: Vec<&str> = BY_EXTENSION.iter().map(|(name, _)| *name).collect();
        let count = seen.len();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), count, "an extension is in the table twice");
    }
}
