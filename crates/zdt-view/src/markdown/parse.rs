//! The markdown a language server or an agent actually emits.
//!
//! Fenced code, ATX headings, thematic breaks, nested bullet and numbered lists with task marks,
//! block quotes, pipe tables and paragraphs. Inside them: code spans, emphasis, strong emphasis,
//! strikethrough and links. Reference links, footnotes, HTML and setext headings are absent. A
//! parser has to be correct before it is complete, and anything unrecognized reads as prose.

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
    List(Vec<ListItem>),
    /// Quoted blocks.
    Quote(Vec<Block>),
    /// A pipe table.
    Table(Table),
    /// A rule across the panel.
    Rule,
}

/// One item of a list, and the items nested under it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ListItem {
    /// The number it was written with, when the list is numbered.
    pub number: Option<u64>,
    /// Whether it carries a task mark, and whether that mark is checked.
    pub task: Option<bool>,
    /// What it says.
    pub spans: Vec<Span>,
    /// The items indented under it.
    pub children: Vec<ListItem>,
}

/// A pipe table: a header row, how each column leans, and the body.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Table {
    /// How each column leans, from the delimiter row.
    pub align: Vec<Align>,
    /// The header's cells.
    pub head: Vec<Vec<Span>>,
    /// The body's rows, each padded to the header's width.
    pub rows: Vec<Vec<Vec<Span>>>,
}

/// Which way a table column leans.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Align {
    /// Against the left edge, which is also what an unmarked column does.
    Left,
    /// In the middle.
    Center,
    /// Against the right edge.
    Right,
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
    /// Words that are crossed out.
    Strike(Vec<Span>),
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

        // A quote: every consecutive `>` line, stripped of one mark and read again. A quote
        // nested deeper keeps its marks and comes out through the recursion.
        if trimmed.starts_with('>') {
            let start = at;
            while at < lines.len() && lines[at].trim_start().starts_with('>') {
                at += 1;
            }
            let inner = lines[start..at]
                .iter()
                .map(|line| {
                    let bare = line.trim_start();
                    let bare = bare.strip_prefix('>').unwrap_or(bare);
                    bare.strip_prefix(' ').unwrap_or(bare)
                })
                .collect::<Vec<_>>()
                .join("\n");
            blocks.push(Block::Quote(parse(&inner)));
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

        // A table: a row of cells over a delimiter row. Anything short of that pair is prose
        // with pipes in it.
        if trimmed.contains('|')
            && let Some(align) = lines.get(at + 1).and_then(|line| delimiter_of(line.trim()))
        {
            let head_cells = split_row(trimmed);
            if head_cells.len() == align.len() {
                let head = head_cells.iter().map(|cell| spans(cell)).collect();
                at += 2;
                let mut rows = Vec::new();
                while at < lines.len() {
                    let row = lines[at].trim();
                    if row.is_empty() || !row.contains('|') {
                        break;
                    }
                    let mut cells = split_row(row);
                    cells.resize(align.len(), String::new());
                    rows.push(cells.iter().map(|cell| spans(cell)).collect());
                    at += 1;
                }
                blocks.push(Block::Table(Table { align, head, rows }));
                continue;
            }
        }

        if item_of(trimmed).is_some() {
            let mut raws: Vec<RawItem> = Vec::new();
            while at < lines.len() {
                let raw = lines[at];
                let bare = raw.trim();
                if bare.is_empty() {
                    break;
                }
                if let Some((number, rest)) = item_of(bare) {
                    let (task, rest) = task_of(rest);
                    raws.push(RawItem {
                        indent: indent_of(raw),
                        number,
                        task,
                        text: rest.trim().to_owned(),
                    });
                    at += 1;
                    continue;
                }
                // Prose indented under the last item continues that item.
                if let Some(last) = raws.last_mut()
                    && indent_of(raw) > last.indent
                    && fence_of(bare).is_none()
                    && heading_of(bare).is_none()
                    && !is_rule(bare)
                {
                    last.text.push(' ');
                    last.text.push_str(bare);
                    at += 1;
                    continue;
                }
                break;
            }
            blocks.push(Block::List(nested(&raws)));
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
                || item_of(next).is_some()
                || next.starts_with('>')
                || (next.contains('|')
                    && lines
                        .get(at + 1)
                        .is_some_and(|line| delimiter_of(line.trim()).is_some()))
            {
                break;
            }
            at += 1;
        }
        // At least the line itself, so a line that looks like a table head without being one
        // still moves the cursor.
        if at == start {
            at = start + 1;
        }
        // Joined with a space, and never a newline. A hard-wrapped paragraph is one paragraph,
        // and a panel narrower than the wrapping would show it as ragged lines.
        let text = lines[start..at]
            .iter()
            .map(|line| line.trim())
            .collect::<Vec<_>>()
            .join(" ");
        blocks.push(Block::Paragraph(spans(&text)));
    }

    blocks
}

/// One list line before nesting: how deep it sits, and what it says.
struct RawItem {
    indent: usize,
    number: Option<u64>,
    task: Option<bool>,
    text: String,
}

/// Builds the tree the indentation describes.
fn nested(raws: &[RawItem]) -> Vec<ListItem> {
    let mut out = Vec::new();
    let Some(first) = raws.first() else {
        return out;
    };
    let level = first.indent;
    let mut at = 0;
    while at < raws.len() {
        let start = at;
        at += 1;
        while at < raws.len() && raws[at].indent > level {
            at += 1;
        }
        let raw = &raws[start];
        out.push(ListItem {
            number: raw.number,
            task: raw.task,
            spans: spans(&raw.text),
            children: nested(&raws[start + 1..at]),
        });
    }
    out
}

/// How far a line is indented, counting a tab as two.
fn indent_of(line: &str) -> usize {
    line.chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .map(|c| if c == '\t' { 2 } else { 1 })
        .sum()
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

/// The number an item was written with and what it says, when the line is one.
fn item_of(line: &str) -> Option<(Option<u64>, &str)> {
    for mark in ["- ", "* ", "+ "] {
        if let Some(rest) = line.strip_prefix(mark) {
            return Some((None, rest.trim_start()));
        }
    }
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 && digits <= 9 {
        let rest = &line[digits..];
        for mark in [". ", ") "] {
            if let Some(rest) = rest.strip_prefix(mark) {
                return Some((line[..digits].parse().ok(), rest.trim_start()));
            }
        }
    }
    None
}

/// The task mark at the front of an item, when there is one.
fn task_of(rest: &str) -> (Option<bool>, &str) {
    if let Some(rest) = rest.strip_prefix("[ ] ") {
        return (Some(false), rest);
    }
    if let Some(rest) = rest
        .strip_prefix("[x] ")
        .or_else(|| rest.strip_prefix("[X] "))
    {
        return (Some(true), rest);
    }
    (None, rest)
}

/// How a delimiter row says each column leans, when the line is one.
fn delimiter_of(line: &str) -> Option<Vec<Align>> {
    if !line.contains('-') || !line.contains('|') && !line.starts_with(':') {
        return None;
    }
    let cells = split_row(line);
    if cells.is_empty() {
        return None;
    }
    let mut align = Vec::new();
    for cell in &cells {
        let cell = cell.trim();
        let left = cell.starts_with(':');
        let right = cell.ends_with(':');
        let dashes = cell.trim_start_matches(':').trim_end_matches(':');
        if dashes.is_empty() || !dashes.chars().all(|c| c == '-') {
            return None;
        }
        align.push(match (left, right) {
            (true, true) => Align::Center,
            (false, true) => Align::Right,
            _ => Align::Left,
        });
    }
    Some(align)
}

/// One row's cells, outer pipes shed and `\|` kept as a pipe.
fn split_row(line: &str) -> Vec<String> {
    let trimmed = line.trim();
    let trimmed = trimmed.strip_prefix('|').unwrap_or(trimmed);
    let mut cells = Vec::new();
    let mut cell = String::new();
    let mut chars = trimmed.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some('|') => cell.push('|'),
                Some(next) => {
                    cell.push('\\');
                    cell.push(next);
                }
                None => cell.push('\\'),
            },
            '|' => cells.push(std::mem::take(&mut cell)),
            _ => cell.push(c),
        }
    }
    if !cell.trim().is_empty() || !trimmed.ends_with('|') {
        cells.push(cell);
    }
    cells
        .into_iter()
        .map(|cell| cell.trim().to_owned())
        .collect()
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

        // Two tildes cross words out; one is a tilde.
        if here == '~'
            && run_of(&bytes, at, '~') >= 2
            && let Some(close) = find_run(&bytes, at + 2, '~', 2)
        {
            let inner: String = bytes[at + 2..close].iter().collect();
            if !inner.is_empty() {
                flush!();
                out.push(Span::Strike(spans(&inner)));
                at = close + 2;
                continue;
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

/// Where the next run of at least `length` `mark`s starts, at or after `from`.
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

#[cfg(test)]
mod tests {
    use super::{Align, Block, ListItem, Span, parse, spans};

    /// The spans of a paragraph, for the assertions that are about one line.
    fn line(text: &str) -> Vec<Span> {
        spans(text)
    }

    /// A plain item with nothing nested under it.
    fn item(text: &str) -> ListItem {
        ListItem {
            number: None,
            task: None,
            spans: line(text),
            children: Vec::new(),
        }
    }

    #[test]
    fn a_fence_keeps_its_language_and_loses_its_marks() {
        // That is the whole reason for parsing. The language is what makes the signature
        // highlighted, and it is written on the fence.
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
        for text in ["- one\n- two", "* one\n* two", "+ one\n+ two"] {
            assert_eq!(
                parse(text),
                vec![Block::List(vec![item("one"), item("two")])],
                "{text}"
            );
        }
    }

    #[test]
    fn a_numbered_list_keeps_its_numbers() {
        let blocks = parse("3. three\n4. four");
        let Block::List(items) = &blocks[0] else {
            panic!("a list");
        };
        assert_eq!(items[0].number, Some(3));
        assert_eq!(items[1].number, Some(4));
    }

    #[test]
    fn an_indented_item_nests_under_the_one_before_it() {
        let blocks = parse("- outer\n  - inner one\n  - inner two\n- next");
        let Block::List(items) = &blocks[0] else {
            panic!("a list");
        };
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].spans, line("outer"));
        assert_eq!(items[0].children.len(), 2);
        assert_eq!(items[0].children[1].spans, line("inner two"));
        assert!(items[1].children.is_empty());
    }

    #[test]
    fn a_task_mark_is_read_off_the_item() {
        let blocks = parse("- [ ] open\n- [x] done");
        let Block::List(items) = &blocks[0] else {
            panic!("a list");
        };
        assert_eq!(items[0].task, Some(false));
        assert_eq!(items[0].spans, line("open"));
        assert_eq!(items[1].task, Some(true));
    }

    #[test]
    fn a_wrapped_item_continues_on_the_indented_line() {
        let blocks = parse("- first line\n  and its continuation");
        let Block::List(items) = &blocks[0] else {
            panic!("a list");
        };
        assert_eq!(items[0].spans, line("first line and its continuation"));
    }

    #[test]
    fn a_quote_holds_whole_blocks() {
        let blocks = parse("> quoted words\n> across lines");
        assert_eq!(
            blocks,
            vec![Block::Quote(vec![Block::Paragraph(line(
                "quoted words across lines"
            ))])]
        );
    }

    #[test]
    fn a_table_keeps_its_header_its_leanings_and_its_rows() {
        let blocks = parse("| Name | Count |\n|:-----|------:|\n| a | 1 |\n| b | 2 |");
        let Block::Table(table) = &blocks[0] else {
            panic!("a table, not {blocks:?}");
        };
        assert_eq!(table.align, vec![Align::Left, Align::Right]);
        assert_eq!(table.head, vec![line("Name"), line("Count")]);
        assert_eq!(table.rows.len(), 2);
        assert_eq!(table.rows[1], vec![line("b"), line("2")]);
    }

    #[test]
    fn a_short_table_row_is_padded_to_the_header() {
        let blocks = parse("| a | b |\n|---|---|\n| only |");
        let Block::Table(table) = &blocks[0] else {
            panic!("a table");
        };
        assert_eq!(table.rows[0].len(), 2);
        assert!(table.rows[0][1].is_empty());
    }

    #[test]
    fn a_lone_pipe_in_prose_is_prose() {
        assert_eq!(
            parse("either | or"),
            vec![Block::Paragraph(line("either | or"))]
        );
    }

    #[test]
    fn an_escaped_pipe_stays_inside_its_cell() {
        let blocks = parse("| a \\| b |\n|---|");
        let Block::Table(table) = &blocks[0] else {
            panic!("a table");
        };
        assert_eq!(table.head, vec![line("a | b")]);
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
    fn two_tildes_cross_words_out_and_one_is_a_tilde() {
        assert_eq!(
            spans("keep ~~drop~~ keep"),
            vec![
                Span::Text("keep ".to_owned()),
                Span::Strike(vec![Span::Text("drop".to_owned())]),
                Span::Text(" keep".to_owned()),
            ]
        );
        assert_eq!(spans("~/.config"), vec![Span::Text("~/.config".to_owned())]);
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
