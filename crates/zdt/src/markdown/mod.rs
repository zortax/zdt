//! Markdown, as a documentation panel wants it.
//!
//! Every language server answers `textDocument/hover` and `completionItem/resolve` with markdown.
//! This editor once threw the markup away and showed the text. That is defensible for one line in
//! the status line, and indefensible for a panel. A hover from `rust-analyzer` is a signature in a
//! fenced block, a rule, a paragraph of prose, and a bulleted list of the trait bounds. Shown as
//! plain text, those four things look like one thing.
//!
//! So this parses it. It parses the subset that language servers emit, which is a much smaller
//! language than CommonMark: fenced code, ATX headings, thematic breaks, bullet and numbered
//! lists, and paragraphs. Inside them: code spans, emphasis, strong emphasis and links. Tables,
//! block quotes, reference links, footnotes, HTML and setext headings are absent. No server has
//! sent one in a hover, and a parser has to be correct before it is complete.
//!
//! # The parser
//!
//! `pulldown-cmark` returns an event stream. This module must still make blocks, spans and views
//! from it, and that work is two thirds of the code below. The parser is the other third.
//!
//! # The code blocks
//!
//! A fenced block is a mounted editor. That is the only way to get the fence highlighted as the
//! language it says it is, and it costs nothing that is not already being paid. The picker's
//! preview does exactly this, for the same reason.

mod parse;
mod view;

pub use crate::markdown::parse::{Block, Span, parse, spans};
pub use crate::markdown::view::{Markdown, MarkdownProps};
