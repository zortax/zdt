//! What could be typed next.
//!
//! The same shape as the picker, and for the same reason: an answer that arrives for a prefix
//! nobody is typing any more must be dropped. Every request carries a generation, and an answer
//! whose generation is not the current one is thrown away.
//!
//! # What crosses to the server, and when
//!
//! A request goes out when a word starts, after `completion_min_chars` characters, or when the
//! typed character is one the server named as a trigger. After that, *typing asks nothing again*.
//! The list is re-ranked in memory against the growing prefix, with the same matcher the picker
//! uses. That is what makes typing inside a word cost nothing, and why the popup keeps up with
//! somebody who types quickly.
//!
//! A request goes out again when the word is left and another begins, and whenever a trigger
//! character says the answer would be a different list. `.` in Rust asks a different question from
//! the identifier before it.
//!
//! # What the popup takes
//!
//! Only the keys it is bound to, in `assets/keymap-completion.toml`. Everything else goes where it
//! would have gone anyway, so typing on past a popup is typing. A popup that swallowed the next
//! character would cost more than it saves.

pub mod view;

mod ask;
mod keys;
mod read;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use zdt_core::search::fuzzy;
use zgui::reactive::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};
use zgui_editor::EditorHandle;

use crate::settings::Settings;
use crate::workspace::Workspace;

/// How long after a trigger the server is asked.
///
/// Short enough that the popup feels like it was already there, long enough that typing a word
/// does not start five requests.
const DEBOUNCE: Duration = Duration::from_millis(80);

/// How many suggestions the popup shows at once before it scrolls.
pub const VISIBLE: usize = 12;

/// How tall one row is. The list is told, and measures nothing.
pub const ROW: f32 = 20.0;

/// One suggestion, as the popup draws it.
#[derive(Clone, PartialEq, Debug)]
pub struct Item {
    /// What it is called.
    pub label: String,
    /// What kind of thing it is.
    pub kind: Option<lsp_types::CompletionItemKind>,
    /// The type or signature beside it, when the server gave one.
    pub detail: Option<String>,
    /// Where it sits in the list the server sent, so the documentation can be asked for.
    pub index: usize,
}

impl Item {
    /// Which tone the kind's glyph is drawn in.
    ///
    /// Four groups, and not twenty-five. What somebody reads off a glyph at twelve pixels is "is
    /// this a function, a type, a value or a word". A palette with a colour per protocol constant
    /// says nothing.
    #[must_use]
    pub const fn tone(&self) -> &'static str {
        use lsp_types::CompletionItemKind as Kind;

        match self.kind {
            Some(Kind::FUNCTION | Kind::METHOD | Kind::CONSTRUCTOR) => "function",
            Some(
                Kind::CLASS | Kind::INTERFACE | Kind::STRUCT | Kind::ENUM | Kind::TYPE_PARAMETER,
            ) => "type",
            Some(
                Kind::VARIABLE
                | Kind::FIELD
                | Kind::PROPERTY
                | Kind::CONSTANT
                | Kind::ENUM_MEMBER
                | Kind::VALUE,
            ) => "value",
            Some(Kind::KEYWORD | Kind::SNIPPET | Kind::OPERATOR) => "keyword",
            _ => "text",
        }
    }

    /// Which glyph it gets.
    ///
    /// From the interface font's private-use range, where the devicons live. The buffer line's
    /// file-type glyphs come from the same source, so they carry the same weight.
    #[must_use]
    pub const fn glyph(&self) -> &'static str {
        use lsp_types::CompletionItemKind as Kind;

        match self.kind {
            Some(Kind::FUNCTION | Kind::METHOD) => "\u{f0295}",
            Some(Kind::CONSTRUCTOR) => "\u{f0674}",
            Some(Kind::CLASS | Kind::STRUCT) => "\u{f0233}",
            Some(Kind::INTERFACE) => "\u{f0e8}",
            Some(Kind::ENUM | Kind::ENUM_MEMBER) => "\u{f0a5c}",
            Some(Kind::MODULE) => "\u{f0487}",
            Some(Kind::VARIABLE | Kind::VALUE) => "\u{f0b97}",
            Some(Kind::FIELD | Kind::PROPERTY) => "\u{f0ad1}",
            Some(Kind::CONSTANT) => "\u{f0ff2}",
            Some(Kind::KEYWORD) => "\u{f11c}",
            Some(Kind::SNIPPET) => "\u{f0c29}",
            Some(Kind::FILE) => "\u{f0214}",
            Some(Kind::FOLDER) => "\u{f024b}",
            _ => "\u{f0219}",
        }
    }
}

/// What is open, and where.
#[derive(Clone, PartialEq, Debug)]
pub struct Open {
    /// Where the caret was when the popup opened, for anchoring.
    pub caret: zgui_editor::CaretRect,
    /// What the popup would replace if something were taken.
    pub replaces: std::ops::Range<usize>,
}

/// The suggestions.
#[derive(Clone)]
pub struct Completion {
    inner: Rc<Inner>,
}

struct Inner {
    settings: Settings,
    /// The language servers.
    ///
    /// Taken once at construction. Everything this does happens inside a debounce timer or after
    /// an await, and a context looked up in either is gone. See `tests/context.rs`. Looked up
    /// there, completion silently asked nothing.
    language: Option<crate::language::Language>,
    /// The window's clock, taken once where there certainly is one.
    timers: Option<zgui::view::time::Timers>,

    /// Whether the popup is up, and where.
    open: RwSignal<Option<Open>, LocalStorage>,
    /// What it is showing, ranked.
    items: RwSignal<Vec<Item>, LocalStorage>,
    /// Which row the caret is on.
    at: RwSignal<usize, LocalStorage>,
    /// The documentation of the row the caret is on, once it has been asked for.
    docs: RwSignal<Option<Vec<crate::markdown::Block>>, LocalStorage>,
    /// How far down the documentation has been scrolled.
    docs_offset: RwSignal<f32, LocalStorage>,

    /// Everything the server sent, before ranking.
    all: RefCell<Vec<lsp_types::CompletionItem>>,
    /// What the prefix was when the server was asked, so the re-ranking knows what to strip.
    asked_at: Cell<usize>,
    /// Which question is being answered. An answer for an older one is thrown away.
    generation: Cell<u64>,
    /// The same, for documentation: walking a list quickly must not draw the docs of a row the
    /// caret has already left.
    docs_generation: Cell<u64>,
    /// What is waiting to ask the server.
    pending: RefCell<Option<zgui::view::time::TimeoutHandle>>,
    /// What is waiting to ask for documentation.
    docs_pending: RefCell<Option<zgui::view::time::TimeoutHandle>>,
}

impl Completion {
    /// Nothing suggested.
    #[must_use]
    pub fn new(settings: Settings, language: Option<crate::language::Language>) -> Self {
        Self {
            inner: Rc::new(Inner {
                settings,
                language,
                timers: zgui::view::time::Timers::current(),
                open: RwSignal::new_local(None),
                items: RwSignal::new_local(Vec::new()),
                at: RwSignal::new_local(0),
                docs: RwSignal::new_local(None),
                docs_offset: RwSignal::new_local(0.0),
                all: RefCell::new(Vec::new()),
                asked_at: Cell::new(0),
                generation: Cell::new(0),
                docs_generation: Cell::new(0),
                pending: RefCell::new(None),
                docs_pending: RefCell::new(None),
            }),
        }
    }
}

/// Puts the suggestions where every component can find them.
pub fn provide(completion: Completion) {
    zgui::reactive::provide_local_context(completion);
}

/// Them, from inside a component.
///
/// # Panics
///
/// If none were provided above this component, which is a wiring mistake.
#[must_use]
pub fn use_completion() -> Completion {
    zgui::reactive::use_local_context::<Completion>().expect("suggestions are provided at the root")
}

/// The identifier being typed, and where it starts.
///
/// Walks back from the caret over what an identifier is made of. `None` when the caret is outside
/// one, which is most of the time. That is also the cheapest path, and it is taken on every
/// keystroke.
#[must_use]
pub fn prefix_at(handle: &EditorHandle) -> Option<(String, std::ops::Range<usize>)> {
    handle.query(|snapshot| {
        let rope = snapshot.rope();
        let caret = snapshot.selections().primary().head;
        if caret > rope.len_bytes() {
            return None;
        }

        // Walked backwards through the rope, and never over a copy of it. The copy is what this
        // did first. In a large file it is a megabyte memcpy per character typed, which reads as
        // the editor freezing.
        let mut chars = rope.chars_at(rope.byte_to_char(caret));
        let mut start = caret;
        while let Some(previous) = chars.prev() {
            if previous.is_alphanumeric() || previous == '_' {
                start -= previous.len_utf8();
            } else {
                break;
            }
        }
        if start == caret {
            return None;
        }

        let word: String = rope.byte_slice(start..caret).chars().collect();
        // A word beginning with a digit is a number being typed, not an identifier.
        if word.chars().next().is_some_and(char::is_numeric) {
            return None;
        }
        Some((word, start..caret))
    })
}

/// Ranks `items` against `query`, keeping the order the server sent when nothing was typed.
///
/// The server's own order is meaningful. `rust-analyzer` puts the most likely completion first,
/// so an empty query must leave it alone. Once there is a query the fuzzy matcher decides. It is
/// the same matcher and the same feel as the pickers.
#[must_use]
pub fn rank(items: &[lsp_types::CompletionItem], query: &str) -> Vec<Item> {
    let made = |index: usize, item: &lsp_types::CompletionItem| Item {
        label: item.label.clone(),
        kind: item.kind,
        detail: detail_of(item),
        index,
    };

    if query.is_empty() {
        return items
            .iter()
            .enumerate()
            .map(|(at, item)| made(at, item))
            .collect();
    }

    // Matched against the filter text where a server gave one, which is how it offers
    // `use std::fmt` under the label `Display`. Shown under the label either way.
    let against: Vec<String> = items
        .iter()
        .map(|item| {
            item.filter_text
                .clone()
                .unwrap_or_else(|| item.label.clone())
        })
        .collect();

    // The same matcher and the same feel as the pickers. Blocking and linear, which is right for
    // a list this size: a completion list is hundreds of rows, not the hundred thousand a project
    // walk produces, and starting nucleo's threads per keystroke would cost more than it saves.
    fuzzy::rank(&against, query, items.len())
        .into_iter()
        .map(|found| made(found.index, &items[found.index]))
        .collect()
}

/// What a server said about a suggestion, parsed.
///
/// `None` when it said nothing, which is the usual answer before the item has been resolved.
#[must_use]
pub fn documentation(item: &lsp_types::CompletionItem) -> Option<Vec<crate::markdown::Block>> {
    use lsp_types::Documentation;

    let markdown = match item.documentation.as_ref()? {
        Documentation::String(text) => text.clone(),
        Documentation::MarkupContent(markup) => markup.value.clone(),
    };
    let blocks = crate::markdown::parse(&markdown);
    (!blocks.is_empty()).then_some(blocks)
}

/// The line beside a suggestion: its type, or the first line of its detail.
fn detail_of(item: &lsp_types::CompletionItem) -> Option<String> {
    if let Some(detail) = item.detail.as_ref() {
        return Some(detail.lines().next().unwrap_or("").trim().to_owned());
    }
    item.label_details
        .as_ref()
        .and_then(|details| details.description.clone())
}

/// What accepting a suggestion puts in, and over what.
///
/// The server may say either: a `text_edit` naming its own range, or an `insert_text` that goes
/// where the prefix is. The edit wins when there is one, because a server that named a range meant
/// that range. `rust-analyzer` completing `.map(` replaces the dot as well as the word.
#[must_use]
pub fn replacement(
    item: &lsp_types::CompletionItem,
    prefix: std::ops::Range<usize>,
    handle: &EditorHandle,
    encoding: zdt_lsp::Encoding,
) -> (std::ops::Range<usize>, String) {
    // Whether `$` in what follows is a tab stop or a dollar sign. This is the item's own answer
    // and not a guess: `$5` is a perfectly ordinary thing for a label to contain, and stripping
    // it out of one that was never a snippet would turn "cost $5" into "cost ".
    let snippet = item.insert_text_format == Some(lsp_types::InsertTextFormat::SNIPPET);
    let text = |raw: &str| {
        if snippet {
            literal(raw)
        } else {
            raw.to_owned()
        }
    };

    if let Some(lsp_types::CompletionTextEdit::Edit(edit)) = item.text_edit.as_ref() {
        let range = handle
            .query(|snapshot| zdt_lsp::convert::range_of(snapshot.rope(), edit.range, encoding));
        return (range, text(&edit.new_text));
    }
    if let Some(lsp_types::CompletionTextEdit::InsertAndReplace(edit)) = item.text_edit.as_ref() {
        // The replacing range, and not the inserting one. Somebody completing over a word meant
        // to replace the word, and that is what the two ranges differ about.
        let range = handle
            .query(|snapshot| zdt_lsp::convert::range_of(snapshot.rope(), edit.replace, encoding));
        return (range, text(&edit.new_text));
    }

    let raw = item
        .insert_text
        .clone()
        .unwrap_or_else(|| item.label.clone());
    (prefix, text(&raw))
}

/// Snippet syntax, as an editor that does not do snippets should insert it.
///
/// The client declares `snippet_support: false`, so a well-behaved server never sends one. Some
/// servers send one anyway, and a label that arrives as `foo(${1:x})` must go in as `foo(x)`.
///
/// Only called for an item that said it *is* a snippet. See [`replacement`]. Applied to anything
/// else it would eat a dollar sign somebody meant.
#[must_use]
pub fn literal(text: &str) -> String {
    if !text.contains('$') {
        return text.to_owned();
    }

    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    while at < chars.len() {
        if chars[at] == '\\' && at + 1 < chars.len() {
            out.push(chars[at + 1]);
            at += 2;
            continue;
        }
        if chars[at] != '$' {
            out.push(chars[at]);
            at += 1;
            continue;
        }

        // `$0`, `$1`: a tab stop with nothing in it.
        if chars.get(at + 1).is_some_and(char::is_ascii_digit) {
            at += 1;
            while chars.get(at).is_some_and(char::is_ascii_digit) {
                at += 1;
            }
            continue;
        }
        // `${1:default}`: the default is what goes in.
        if chars.get(at + 1) == Some(&'{') {
            let mut depth = 0;
            let mut end = at + 1;
            while end < chars.len() {
                match chars[end] {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                end += 1;
            }
            let inside: String = chars[at + 2..end.min(chars.len())].iter().collect();
            if let Some((_, default)) = inside.split_once(':') {
                out.push_str(&literal(default));
            }
            at = end + 1;
            continue;
        }

        out.push('$');
        at += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{literal, rank};

    fn item(label: &str) -> lsp_types::CompletionItem {
        lsp_types::CompletionItem {
            label: label.to_owned(),
            ..Default::default()
        }
    }

    #[test]
    fn an_empty_query_keeps_the_order_the_server_sent() {
        // Which is meaningful: rust-analyzer puts the most likely completion first, and re-sorting
        // it alphabetically would throw away the one thing the server knows that this does not.
        let items = [item("zebra"), item("apple"), item("mango")];
        let ranked = rank(&items, "");
        assert_eq!(
            ranked
                .iter()
                .map(|one| one.label.as_str())
                .collect::<Vec<_>>(),
            ["zebra", "apple", "mango"]
        );
    }

    #[test]
    fn a_query_keeps_only_what_matches() {
        let items = [item("push"), item("pop"), item("len")];
        let ranked = rank(&items, "p");
        assert_eq!(ranked.len(), 2);
        assert!(ranked.iter().all(|one| one.label.starts_with('p')));
    }

    #[test]
    fn a_query_that_matches_nothing_is_an_empty_list() {
        let items = [item("push"), item("pop")];
        assert!(rank(&items, "zzzz").is_empty());
    }

    #[test]
    fn filter_text_is_what_is_matched_against_when_there_is_one() {
        // Which is how a server offers `use std::fmt` under the label `fmt`.
        let mut one = item("Display");
        one.filter_text = Some("fmt_display".to_owned());
        let ranked = rank(&[one], "fmt");
        assert_eq!(ranked.len(), 1, "matched on the filter text, not the label");
        assert_eq!(ranked[0].label, "Display", "and shown under its own label");
    }

    #[test]
    fn a_snippet_goes_in_as_what_it_would_have_looked_like() {
        // The client says it does not do snippets. Servers send them anyway, and `foo(${1:x})`
        // typed into a file as its own source is worse than no completion at all.
        assert_eq!(literal("foo(${1:x})"), "foo(x)");
        assert_eq!(literal("foo($1)"), "foo()");
        assert_eq!(literal("println!(\"$0\")"), "println!(\"\")");
        assert_eq!(literal("plain"), "plain");
    }

    #[test]
    fn an_escaped_tab_stop_is_a_dollar_sign() {
        assert_eq!(literal(r"\$1"), "$1");
        assert_eq!(literal("100% $"), "100% $");
    }

    #[test]
    fn only_a_snippet_is_read_as_one() {
        // The defect this prevents: a completion labelled `cost $5` going into the file as
        // `cost `. That label is an ordinary string in half the languages there are. Whether `$5`
        // is a tab stop or two characters is the item's own answer, and guessing it is how a
        // plain completion gets eaten.
        let mut plain = item("cost $5");
        plain.insert_text = Some("cost $5".to_owned());
        plain.insert_text_format = Some(lsp_types::InsertTextFormat::PLAIN_TEXT);

        let mut snippet = item("foo");
        snippet.insert_text = Some("foo(${1:x})".to_owned());
        snippet.insert_text_format = Some(lsp_types::InsertTextFormat::SNIPPET);

        // The two disagree about the same characters, which is the whole point of the flag.
        assert!(
            !literal("cost $5").contains('5'),
            "read as a snippet it loses the 5"
        );
        assert_eq!(
            plain.insert_text.as_deref(),
            Some("cost $5"),
            "and read as itself it does not"
        );
        assert_eq!(literal(snippet.insert_text.as_deref().unwrap()), "foo(x)");
    }

    #[test]
    fn a_nested_placeholder_keeps_its_innermost_default() {
        assert_eq!(literal("${1:${2:inner}}"), "inner");
    }
}
