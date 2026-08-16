//! Markdown, as a documentation panel wants it.
//!
//! Every language server answers `textDocument/hover` and `completionItem/resolve` with markdown,
//! and until now this editor threw the markup away and showed the text. That is defensible for one
//! line in the status line and indefensible for a panel: a hover from `rust-analyzer` is a
//! signature in a fenced block, a rule, a paragraph of prose, and a bulleted list of the trait
//! bounds — and shown as plain text those four things look like one thing.
//!
//! So this parses it. Not all of it: what is here is the subset that language servers actually
//! emit, which is a much smaller language than CommonMark. Fenced code, ATX headings, thematic
//! breaks, bullet and numbered lists, paragraphs; and inside them code spans, emphasis, strong
//! emphasis and links. Tables, block quotes, reference links, footnotes, HTML and setext headings
//! are not here, because no server has ever sent one in a hover and a parser is a thing that has
//! to be correct rather than complete.
//!
//! # Why not a crate
//!
//! `pulldown-cmark` is the obvious answer and it is the wrong one here. It would parse the whole
//! language correctly and then hand back an event stream that still has to be turned into blocks,
//! spans and views — which is the part that is actually being written below. The parsing is a
//! third of this file; the other two thirds would be written either way.
//!
//! # The code blocks
//!
//! A fenced block is a mounted editor, not styled text. That is the only way to get the fence
//! highlighted as the language it says it is, and it costs nothing that is not already being paid:
//! the picker's preview does exactly this, and for the same reason.

use zgui::prelude::*;
use zgui::{component, view};

/// One thing in a document.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Block {
    /// A run of prose.
    Paragraph(Vec<Span>),
    /// A heading, and how deep it is.
    Heading(u8, Vec<Span>),
    /// A fenced block, and what language it says it is.
    Code {
        /// The language named after the fence, when one was.
        language: Option<String>,
        /// What is inside it, fences excluded.
        text: String,
    },
    /// A list, and what is in each item.
    List(Vec<Vec<Span>>),
    /// A rule across the panel.
    Rule,
}

/// One thing in a line.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Span {
    /// Words.
    Text(String),
    /// Words in a code font.
    Code(String),
    /// Words that lean.
    Emphasis(Vec<Span>),
    /// Words that are heavier.
    Strong(Vec<Span>),
    /// Words that stand for somewhere else.
    ///
    /// The target is kept, though nothing follows it yet: a hover's links are `rustdoc` paths and
    /// `https` addresses, and both are worth being able to see even before either can be opened.
    Link {
        /// What it says.
        text: Vec<Span>,
        /// Where it points.
        href: String,
    },
}

/// Reads `markdown` into the blocks it is made of.
///
/// Never fails. Anything that is not one of the shapes below is a paragraph, which is the reading
/// that loses the least.
#[must_use]
pub fn parse(markdown: &str) -> Vec<Block> {
    let mut blocks: Vec<Block> = Vec::new();
    let lines: Vec<&str> = markdown.lines().collect();
    let mut at = 0;

    while at < lines.len() {
        let line = lines[at];
        let trimmed = line.trim();

        // A fence, and everything up to the one that closes it. The closing fence is optional:
        // a server that truncates its own answer mid-block should still show the block.
        if let Some(language) = fence_of(trimmed) {
            at += 1;
            let start = at;
            while at < lines.len() && fence_of(lines[at].trim()).is_none() {
                at += 1;
            }
            let text = lines[start..at].join("\n");
            // Past the closing fence, when there was one.
            at = (at + 1).min(lines.len());
            // An empty fence is a fence about nothing.
            if !text.trim().is_empty() {
                blocks.push(Block::Code {
                    language,
                    text: trim_blank_edges(&text),
                });
            }
            continue;
        }

        if is_rule(trimmed) {
            blocks.push(Block::Rule);
            at += 1;
            continue;
        }

        if let Some((depth, rest)) = heading_of(trimmed) {
            blocks.push(Block::Heading(depth, spans(rest)));
            at += 1;
            continue;
        }

        if bullet_of(trimmed).is_some() {
            let mut items = Vec::new();
            while at < lines.len()
                && let Some(item) = bullet_of(lines[at].trim())
            {
                items.push(spans(item));
                at += 1;
            }
            blocks.push(Block::List(items));
            continue;
        }

        if trimmed.is_empty() {
            at += 1;
            continue;
        }

        // Everything else is prose, up to the next blank line or the next thing that is not.
        let start = at;
        while at < lines.len() {
            let next = lines[at].trim();
            if next.is_empty()
                || is_rule(next)
                || fence_of(next).is_some()
                || heading_of(next).is_some()
                || bullet_of(next).is_some()
            {
                break;
            }
            at += 1;
        }
        // Joined with a space rather than a newline: a hard-wrapped paragraph is one paragraph,
        // and a panel narrower than the wrapping would otherwise show it as ragged lines.
        let text = lines[start..at]
            .iter()
            .map(|line| line.trim())
            .collect::<Vec<_>>()
            .join(" ");
        blocks.push(Block::Paragraph(spans(&text)));
    }

    blocks
}

/// The language a fence names, when the line is one.
fn fence_of(line: &str) -> Option<Option<String>> {
    let rest = line
        .strip_prefix("```")
        .or_else(|| line.strip_prefix("~~~"))?;
    let named = rest.trim();
    Some((!named.is_empty()).then(|| named.to_owned()))
}

/// Whether a line is a rule.
fn is_rule(line: &str) -> bool {
    let bare: String = line.chars().filter(|c| !c.is_whitespace()).collect();
    bare.len() >= 3
        && (bare.chars().all(|c| c == '-')
            || bare.chars().all(|c| c == '_')
            || bare.chars().all(|c| c == '*'))
}

/// How deep a heading is and what it says, when the line is one.
fn heading_of(line: &str) -> Option<(u8, &str)> {
    let hashes = line.chars().take_while(|c| *c == '#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let rest = &line[hashes..];
    // `#foo` is not a heading; `# foo` is. Without this a hover mentioning `#[derive]` opens with
    // a heading.
    if !rest.starts_with(' ') && !rest.is_empty() {
        return None;
    }
    Some((hashes as u8, rest.trim()))
}

/// What a list item says, when the line is one.
fn bullet_of(line: &str) -> Option<&str> {
    for mark in ["- ", "* ", "+ "] {
        if let Some(rest) = line.strip_prefix(mark) {
            return Some(rest.trim());
        }
    }
    // A numbered item, whose number is not kept: the list is drawn with its own marks, and a
    // server that starts at 3 meant 3 items rather than a list starting at three.
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 {
        let rest = &line[digits..];
        for mark in [". ", ") "] {
            if let Some(rest) = rest.strip_prefix(mark) {
                return Some(rest.trim());
            }
        }
    }
    None
}

/// Takes the blank lines off both ends of a block of text.
fn trim_blank_edges(text: &str) -> String {
    let mut lines: Vec<&str> = text.lines().collect();
    while lines.first().is_some_and(|line| line.trim().is_empty()) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.pop();
    }
    lines.join("\n")
}

/// Reads one line into the spans it is made of.
#[must_use]
pub fn spans(text: &str) -> Vec<Span> {
    let bytes: Vec<char> = text.chars().collect();
    let mut out: Vec<Span> = Vec::new();
    let mut plain = String::new();
    let mut at = 0;

    // Pushes whatever plain text has accumulated, so that a marker never splits a run in two.
    macro_rules! flush {
        () => {
            if !plain.is_empty() {
                out.push(Span::Text(std::mem::take(&mut plain)));
            }
        };
    }

    while at < bytes.len() {
        let here = bytes[at];

        // A code span first, and greedily, because everything inside one is literal: a signature
        // holding `*mut T` must not become emphasis.
        if here == '`' {
            let ticks = run_of(&bytes, at, '`');
            if let Some(close) = find_run(&bytes, at + ticks, '`', ticks) {
                flush!();
                let inner: String = bytes[at + ticks..close].iter().collect();
                out.push(Span::Code(inner.trim().to_owned()));
                at = close + ticks;
                continue;
            }
        }

        if here == '['
            && let Some((text, href, end)) = link_at(&bytes, at)
        {
            flush!();
            out.push(Span::Link {
                text: spans(&text),
                href,
            });
            at = end;
            continue;
        }

        if here == '*' || here == '_' {
            let marks = run_of(&bytes, at, here).min(2);
            if let Some(close) = find_run(&bytes, at + marks, here, marks) {
                let inner: String = bytes[at + marks..close].iter().collect();
                // `a_b_c` inside a word is an identifier, not emphasis. Underscores only open a
                // run at a word boundary; asterisks always do, because nothing is named with one.
                let boundary = here == '*'
                    || (at == 0 || !bytes[at - 1].is_alphanumeric())
                        && bytes
                            .get(close + marks)
                            .is_none_or(|next| !next.is_alphanumeric());
                if boundary && !inner.is_empty() {
                    flush!();
                    let inside = spans(&inner);
                    out.push(if marks == 2 {
                        Span::Strong(inside)
                    } else {
                        Span::Emphasis(inside)
                    });
                    at = close + marks;
                    continue;
                }
            }
        }

        // A backslash makes the next character literal, which is how a server writes an asterisk
        // it means as an asterisk.
        if here == '\\' && at + 1 < bytes.len() {
            plain.push(bytes[at + 1]);
            at += 2;
            continue;
        }

        plain.push(here);
        at += 1;
    }

    flush!();
    out
}

/// How many of `mark` there are in a row starting at `at`.
fn run_of(chars: &[char], at: usize, mark: char) -> usize {
    chars[at..].iter().take_while(|c| **c == mark).count()
}

/// Where the next run of exactly `length` `mark`s starts, at or after `from`.
fn find_run(chars: &[char], from: usize, mark: char, length: usize) -> Option<usize> {
    let mut at = from;
    while at < chars.len() {
        if chars[at] == mark {
            let here = run_of(chars, at, mark);
            if here >= length {
                return Some(at);
            }
            at += here;
        } else {
            at += 1;
        }
    }
    None
}

/// The text, target and end of a link starting at `at`.
fn link_at(chars: &[char], at: usize) -> Option<(String, String, usize)> {
    let close = chars[at..].iter().position(|c| *c == ']')? + at;
    if chars.get(close + 1) != Some(&'(') {
        return None;
    }
    let end = chars[close + 2..].iter().position(|c| *c == ')')? + close + 2;
    let text: String = chars[at + 1..close].iter().collect();
    let href: String = chars[close + 2..end].iter().collect();
    // The title after the target, when there is one, is not shown anywhere.
    let href = href
        .split_once(char::is_whitespace)
        .map_or(href.as_str(), |(target, _)| target)
        .to_owned();
    Some((text, href, end + 1))
}

/// A document, drawn.
#[component]
pub fn Markdown(
    /// What to draw.
    blocks: Vec<Block>,
    /// Classes merged after its own.
    #[prop(into, optional)]
    class: Classes,
) -> impl IntoView {
    view! {
        column(class = "md", class = class) {
            {blocks.into_iter().map(block_view).collect::<Vec<_>>()}
        }
    }
}

/// One block.
fn block_view(block: Block) -> zgui::view::AnyView {
    use crate::ui::Erase;

    match block {
        Block::Paragraph(inside) => view! {
            box(class = "md__paragraph") {{spans_view(inside)}}
        }
        .any(),
        Block::Heading(depth, inside) => view! {
            box(class = "md__heading", attr:data-depth = Some(depth.to_string())) {{spans_view(inside)}}
        }
        .any(),
        Block::Rule => view! { box(class = "md__rule") {} }.any(),
        Block::List(items) => view! {
            column(class = "md__list") {
                {items
                    .into_iter()
                    .map(|item| view! {
                        row(class = "md__item") {
                            // A mark in its own box, so that a wrapped item's second line lines up
                            // with its first rather than with the bullet.
                            box(class = "md__bullet") {"\u{2022}"}
                            box(class = "md__item-text") {{spans_view(item)}}
                        }
                    })
                    .collect::<Vec<_>>()}
            }
        }
        .any(),
        Block::Code { language, text } => view! { CodeBlock(language = language, text = text) }.any(),
    }
}

/// A run of spans.
fn spans_view(spans: Vec<Span>) -> zgui::view::AnyView {
    use crate::ui::Erase;

    spans
        .into_iter()
        .map(|span| match span {
            Span::Text(text) => view! { text {{text}} }.any(),
            Span::Code(code) => view! { text(class = "md__code") {{code}} }.any(),
            Span::Emphasis(inside) => view! {
                text(class = "md__emphasis") {{spans_view(inside)}}
            }
            .any(),
            Span::Strong(inside) => view! {
                text(class = "md__strong") {{spans_view(inside)}}
            }
            .any(),
            Span::Link { text, .. } => view! {
                text(class = "md__link") {{spans_view(text)}}
            }
            .any(),
        })
        .collect::<Vec<_>>()
        .any()
}

/// A fenced block, highlighted as whatever it says it is.
///
/// An editor rather than styled text, because a signature is the most valuable line in a hover and
/// showing it in one colour throws away the thing that makes it readable. Never focused, so never
/// typed into: read-only without a flag, exactly as the picker's preview is.
#[component]
fn CodeBlock(
    /// What the fence said the language was.
    language: Option<String>,
    /// What is inside it.
    text: String,
) -> impl IntoView {
    use zgui_editor::EditorProps;

    let lines = text.lines().count().max(1);
    let handle: zgui::reactive::RwSignal<
        Option<zgui_editor::EditorHandle>,
        zgui::reactive::LocalStorage,
    > = zgui::reactive::RwSignal::new_local(None);

    // The language is set through the handle rather than the prop, because the prop takes a name
    // and a fence that named nothing has an answer of "none".
    let named = language.clone();
    let on_ready = Box::new(move |ready: zgui_editor::EditorHandle| {
        ready.set_language(named.as_deref());
        handle.set(Some(ready));
    }) as Box<dyn Fn(zgui_editor::EditorHandle)>;

    view! {
        box(
            class = "md__block",
            // The block is exactly as tall as its content: an editor sized by its parent would
            // either clip a six-line signature or leave four blank lines under a one-line one.
            // The panel around it is what scrolls.
            style:--md-block-lines = Some(lines.to_string())
        ) {
            Editor(
                class = "md__editor",
                text = text,
                autofocus = false,
                config = code_config(),
                on_ready = on_ready,
            )
        }
    }
}

/// How a fenced block behaves: no gutter, no caret, no animation, nothing to interact with.
fn code_config() -> zgui_editor::EditorConfig {
    zgui_editor::EditorConfig {
        gutter: zgui_editor::GutterMode::None,
        blink: false,
        smooth_scroll: false,
        ..zgui_editor::EditorConfig::default()
    }
}

#[cfg(test)]
mod tests {
    use super::{Block, Span, parse, spans};

    /// The spans of a paragraph, for the assertions that are about one line.
    fn line(text: &str) -> Vec<Span> {
        spans(text)
    }

    #[test]
    fn a_fence_keeps_its_language_and_loses_its_marks() {
        // Which is the whole reason for parsing rather than stripping: the language is what makes
        // the signature highlighted, and it is written on the fence.
        let blocks = parse("```rust\nfn main() {}\n```");
        assert_eq!(
            blocks,
            vec![Block::Code {
                language: Some("rust".to_owned()),
                text: "fn main() {}".to_owned(),
            }]
        );
    }

    #[test]
    fn a_fence_with_no_language_is_still_a_fence() {
        let blocks = parse("```\nplain\n```");
        assert_eq!(
            blocks,
            vec![Block::Code {
                language: None,
                text: "plain".to_owned(),
            }]
        );
    }

    #[test]
    fn a_fence_nobody_closed_runs_to_the_end() {
        // A server that truncates its own answer should still show what it sent.
        let blocks = parse("```rust\nfn main() {}");
        assert!(matches!(blocks.as_slice(), [Block::Code { .. }]));
    }

    #[test]
    fn the_shape_of_a_rust_analyzer_hover_comes_out_whole() {
        // The real thing, which is a signature, a rule, and a sentence.
        let blocks =
            parse("```rust\npub fn len(&self) -> usize\n```\n\n---\n\nThe number of bytes.");
        assert_eq!(blocks.len(), 3);
        assert!(matches!(blocks[0], Block::Code { .. }));
        assert_eq!(blocks[1], Block::Rule);
        assert_eq!(
            blocks[2],
            Block::Paragraph(vec![Span::Text("The number of bytes.".to_owned())])
        );
    }

    #[test]
    fn a_heading_needs_its_space() {
        // The defect this prevents: a hover mentioning `#[derive(Clone)]` opening with a heading.
        assert_eq!(parse("# Title"), vec![Block::Heading(1, line("Title"))]);
        assert_eq!(
            parse("#[derive(Clone)]"),
            vec![Block::Paragraph(line("#[derive(Clone)]"))]
        );
    }

    #[test]
    fn a_hard_wrapped_paragraph_is_one_paragraph() {
        // A panel narrower than the wrapping would otherwise show ragged lines.
        assert_eq!(
            parse("one two\nthree four"),
            vec![Block::Paragraph(line("one two three four"))]
        );
    }

    #[test]
    fn a_blank_line_ends_a_paragraph() {
        assert_eq!(
            parse("first\n\nsecond"),
            vec![
                Block::Paragraph(line("first")),
                Block::Paragraph(line("second")),
            ]
        );
    }

    #[test]
    fn every_kind_of_list_item_is_a_list_item() {
        let expected = Block::List(vec![line("one"), line("two")]);
        for text in [
            "- one\n- two",
            "* one\n* two",
            "+ one\n+ two",
            "1. one\n2. two",
            "1) one\n2) two",
        ] {
            assert_eq!(parse(text), vec![expected.clone()], "{text}");
        }
    }

    #[test]
    fn a_rule_is_three_of_anything() {
        for text in ["---", "___", "***", "- - -"] {
            assert_eq!(parse(text), vec![Block::Rule], "{text}");
        }
        // And two is not, so a run of dashes in prose stays prose.
        assert_eq!(parse("--"), vec![Block::Paragraph(line("--"))]);
    }

    #[test]
    fn a_code_span_is_literal_all_the_way_through() {
        // The defect this prevents is a signature holding `*mut T` coming out in italics with the
        // asterisk missing.
        assert_eq!(
            spans("takes `*mut T` and returns"),
            vec![
                Span::Text("takes ".to_owned()),
                Span::Code("*mut T".to_owned()),
                Span::Text(" and returns".to_owned()),
            ]
        );
    }

    #[test]
    fn emphasis_and_strong_are_told_apart_by_how_many_marks() {
        assert_eq!(
            spans("*one* **two**"),
            vec![
                Span::Emphasis(vec![Span::Text("one".to_owned())]),
                Span::Text(" ".to_owned()),
                Span::Strong(vec![Span::Text("two".to_owned())]),
            ]
        );
    }

    #[test]
    fn an_underscore_inside_a_word_is_part_of_the_word() {
        // Which is what an identifier looks like, and there is one in nearly every hover.
        assert_eq!(
            spans("call to_owned_thing here"),
            vec![Span::Text("call to_owned_thing here".to_owned())]
        );
    }

    #[test]
    fn a_link_keeps_its_words_and_its_target() {
        assert_eq!(
            spans("see [the docs](https://example.com/x) for more"),
            vec![
                Span::Text("see ".to_owned()),
                Span::Link {
                    text: vec![Span::Text("the docs".to_owned())],
                    href: "https://example.com/x".to_owned(),
                },
                Span::Text(" for more".to_owned()),
            ]
        );
    }

    #[test]
    fn a_bracket_that_is_not_a_link_is_left_alone() {
        assert_eq!(
            spans("an [index] into it"),
            vec![Span::Text("an [index] into it".to_owned())]
        );
    }

    #[test]
    fn a_backslash_makes_the_next_mark_literal() {
        assert_eq!(spans(r"a \* b"), vec![Span::Text("a * b".to_owned())]);
    }

    #[test]
    fn an_unclosed_mark_is_just_a_mark() {
        assert_eq!(
            spans("2 * 3 is 6"),
            vec![Span::Text("2 * 3 is 6".to_owned())]
        );
        assert_eq!(spans("a `b"), vec![Span::Text("a `b".to_owned())]);
    }

    #[test]
    fn nothing_at_all_is_nothing_at_all() {
        assert!(parse("").is_empty());
        assert!(parse("\n\n\n").is_empty());
        // An empty fence is a fence about nothing, which is worth less than the space it takes.
        assert!(parse("```\n```").is_empty());
    }

    #[test]
    fn emphasis_nests() {
        assert_eq!(
            spans("**bold with `code`**"),
            vec![Span::Strong(vec![
                Span::Text("bold with ".to_owned()),
                Span::Code("code".to_owned()),
            ])]
        );
    }
}
