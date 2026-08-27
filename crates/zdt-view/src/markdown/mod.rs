//! Markdown, as a documentation panel wants it.
//!
//! Every language server answers `textDocument/hover` and `completionItem/resolve` with markdown.
//! This editor once threw the markup away and showed the text. That is defensible for one line in
//! the status line, and indefensible for a panel. A hover from `rust-analyzer` is a signature in a
//! fenced block, a rule, a paragraph of prose, and a bulleted list of the trait bounds. Shown as
//! plain text, those four things look like one thing.
//!
//! So this parses it. It parses the subset that language servers, agents and files on disk hold,
//! which is a much smaller language than CommonMark: fenced code, ATX headings, thematic breaks,
//! bullet and numbered lists, block quotes, GitHub alerts, `<details>` blocks, pipe tables,
//! images and paragraphs. Inside them: code spans, emphasis, strong emphasis, strikethrough,
//! links and images. Reference links, footnotes, general HTML and setext headings are absent. A
//! parser has to be correct before it is complete.
//!
//! # The parser
//!
//! Hand-rolled and line-based. It must still make blocks, spans and views, and that work is two
//! thirds of the code below. The parser is the other third.
//!
//! # The code blocks
//!
//! A fenced block is a mounted editor. That is the only way to get the fence highlighted as the
//! language it says it is, and it costs nothing that is not already being paid. The picker's
//! preview does exactly this, for the same reason.

mod parse;
mod view;

pub use crate::markdown::parse::{Align, Block, Callout, ListItem, Span, Table, parse, spans};
pub use crate::markdown::view::{
    Base, Markdown, MarkdownProps, Remote, provide_base, provide_remote,
};
