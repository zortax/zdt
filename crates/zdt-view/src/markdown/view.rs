//! A parsed document, drawn.

use std::path::PathBuf;
use std::rc::Rc;

use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};
use zgui::{component, view};

use crate::markdown::{Block, Span};

/// The directory a document's relative image paths resolve against.
///
/// Provided as a context by whatever shows a document that came from a file. A hover and a chat
/// message come from nowhere, provide none, and show an image's words instead.
#[derive(Clone, Debug)]
pub struct Base(pub PathBuf);

/// Puts the base where a document's views can find it.
pub fn provide_base(base: Base) {
    zgui::reactive::provide_local_context(base);
}

/// What a fetcher answers: nothing while the bytes are on their way or the fetch failed, and
/// the file they landed in once they have.
pub type Fetched = RwSignal<Option<PathBuf>, LocalStorage>;

/// What fetches an image from the network, when the application brings one.
///
/// This crate does no I/O of its own, so with no fetcher provided the words stand in for every
/// remote image.
#[derive(Clone)]
pub struct Remote(pub Rc<dyn Fn(&str) -> Fetched>);

/// Puts the fetcher where a document's views can find it.
pub fn provide_remote(remote: Remote) {
    zgui::reactive::provide_local_context(remote);
}

/// Where `src` is on disk, when it is somewhere this window can read.
///
/// An address on the network is not: nothing here fetches, and the words stand in.
fn resolve(src: &str) -> Option<PathBuf> {
    if src.contains("://") {
        return None;
    }
    let path = PathBuf::from(src);
    if path.is_absolute() {
        return Some(path);
    }
    zgui::reactive::use_local_context::<Base>().map(|base| base.0.join(path))
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
    use crate::Erase;

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
        Block::List(items) => items_view(items),
        Block::Quote(inside) => view! {
            column(class = "md__quote") {
                {inside.into_iter().map(block_view).collect::<Vec<_>>()}
            }
        }
        .any(),
        Block::Table(table) => table_view(table),
        Block::Code { language, text } => view! { CodeBlock(language = language, text = text) }.any(),
        Block::Callout { kind, blocks } => {
            // The mark is a glyph, not an icon: this crate draws documents, and a glyph inherits
            // the title's tint for free.
            let mark = match kind {
                crate::markdown::Callout::Note => "\u{24D8}",
                crate::markdown::Callout::Tip => "\u{2606}",
                crate::markdown::Callout::Important => "\u{2757}",
                crate::markdown::Callout::Warning => "\u{26A0}",
                crate::markdown::Callout::Caution => "\u{2716}",
            };
            view! {
                column(class = "md__callout", attr:data-kind = Some(kind.name().to_owned())) {
                    row(class = "md__callout-title") {
                        text(class = "md__callout-mark") {{mark}}
                        text {{kind.label()}}
                    }
                    {blocks.into_iter().map(block_view).collect::<Vec<_>>()}
                }
            }
            .any()
        }
        Block::Details {
            summary,
            blocks,
            open,
        } => view! {
            Details(summary = summary, blocks = blocks, start_open = open)
        }
        .any(),
        Block::Image { alt, src, width } => image_view(&alt, &src, width, true),
    }
}

/// A picture, or its words when the picture cannot be read.
fn image_view(alt: &str, src: &str, width: Option<u32>, block: bool) -> zgui::view::AnyView {
    use crate::Erase;

    let class = if block {
        "md__image"
    } else {
        "md__image md__image--inline"
    };
    let label = alt.to_owned();
    let width = width.map(|width| format!("{width}px"));
    let words = if alt.is_empty() { src } else { alt }.to_owned();

    if let Some(path) = resolve(src) {
        let src = Some(path.to_string_lossy().into_owned());
        return view! {
            image(class = class, src = src, a11y:label = label, style:width = width)
        }
        .any();
    }

    // An address on the network, handed to the fetcher when the application brought one. The
    // words hold the place while the bytes are on their way, and keep it if they never arrive.
    if (src.starts_with("https://") || src.starts_with("http://"))
        && let Some(remote) = zgui::reactive::use_local_context::<Remote>()
    {
        let fetched = (remote.0)(src);
        return view! {
            {(move || match fetched.get() {
                Some(path) => {
                    let src = Some(path.to_string_lossy().into_owned());
                    let (label, width) = (label.clone(), width.clone());
                    view! {
                        image(class = class, src = src, a11y:label = label, style:width = width)
                    }
                    .any()
                }
                None => {
                    let words = words.clone();
                    view! { text(class = "md__image-words") {{words}} }.any()
                }
            })}
        }
        .any();
    }

    // The words, marked as standing in: an address nothing fetches, or a document from nowhere.
    view! {
        text(class = "md__image-words") {{words}}
    }
    .any()
}

/// A `<details>` block: a summary line that opens and closes what it holds.
///
/// The content stays mounted either way and leaves the flow when closed, so opening a block a
/// second time costs nothing.
#[component]
fn Details(
    /// What the closed block shows. Empty reads as "Details".
    summary: Vec<Span>,
    /// What opening it reveals.
    blocks: Vec<Block>,
    /// Whether it starts open.
    start_open: bool,
) -> impl IntoView {
    use crate::Erase;

    let open: RwSignal<bool, LocalStorage> = RwSignal::new_local(start_open);
    let flip = move |event: &mut EventCx<'_, events::PointerDown>| {
        event.stop_propagation();
        open.update(|held| *held = !*held);
    };
    let title = if summary.is_empty() {
        view! { text {"Details"} }.any()
    } else {
        spans_view(summary)
    };

    view! {
        column(class = "md__details", attr:data-open = move || open.get().then(|| "true".to_owned())) {
            control(
                class = "md__details-summary",
                tabindex = Focus::Programmatic,
                a11y:role = Role::Button,
                a11y:label = "Details",
                on:pointer_down = flip
            ) {
                text(class = "md__details-chevron") {{move || if open.get() { "\u{25BE}" } else { "\u{25B8}" }}}
                {title}
            }
            column(
                class = "md__details-body",
                style:display = move || (!open.get()).then(|| "none".to_owned())
            ) {
                {blocks.into_iter().map(block_view).collect::<Vec<_>>()}
            }
        }
    }
}

/// A list's items, and under each one whatever is nested there.
fn items_view(items: Vec<crate::markdown::ListItem>) -> zgui::view::AnyView {
    use crate::Erase;

    view! {
        column(class = "md__list") {
            {items
                .into_iter()
                .map(|item| {
                    // The mark says what the item is: its own number, its task's state, or a
                    // bullet. In a box of its own, so a wrapped item's second line lines up
                    // with its first.
                    let mark = match (item.number, item.task) {
                        (_, Some(true)) => "\u{2611}".to_owned(),
                        (_, Some(false)) => "\u{2610}".to_owned(),
                        (Some(number), None) => format!("{number}."),
                        (None, None) => "\u{2022}".to_owned(),
                    };
                    let numbered = item.number.is_some() && item.task.is_none();
                    let done = (item.task == Some(true)).then(|| "true".to_owned());
                    let children = if item.children.is_empty() {
                        ().any()
                    } else {
                        view! {
                            box(class = "md__nest") {{items_view(item.children)}}
                        }
                        .any()
                    };
                    view! {
                        column(class = "md__entry") {
                            row(class = "md__item", attr:data-done = move || done.clone()) {
                                box(
                                    class = "md__bullet",
                                    attr:data-numbered = numbered.then(|| "true".to_owned())
                                ) {{mark}}
                                box(class = "md__item-text") {{spans_view(item.spans)}}
                            }
                            {children}
                        }
                    }
                })
                .collect::<Vec<_>>()}
        }
    }
    .any()
}

/// A pipe table: a grid of cells, the header a shade heavier.
fn table_view(table: crate::markdown::Table) -> zgui::view::AnyView {
    use crate::Erase;
    use crate::markdown::Align;

    let columns = table.align.len().max(1);
    let template = format!("repeat({columns}, auto)");
    let lean = |align: Align| match align {
        Align::Left => None,
        Align::Center => Some("center".to_owned()),
        Align::Right => Some("end".to_owned()),
    };

    let mut cells: Vec<zgui::view::AnyView> = Vec::new();
    for (cell, align) in table.head.into_iter().zip(table.align.iter().copied()) {
        let leaning = lean(align);
        cells.push(
            view! {
                box(class = "md__th", style:justify-self = move || leaning.clone()) {{spans_view(cell)}}
            }
            .any(),
        );
    }
    for row in table.rows {
        for (cell, align) in row.into_iter().zip(table.align.iter().copied()) {
            let leaning = lean(align);
            cells.push(
                view! {
                    box(class = "md__td", style:justify-self = move || leaning.clone()) {{spans_view(cell)}}
                }
                .any(),
            );
        }
    }

    view! {
        box(class = "md__tablewrap") {
            box(class = "md__table", style:grid-template-columns = move || Some(template.clone())) {
                {cells}
            }
        }
    }
    .any()
}

/// A run of spans.
fn spans_view(spans: Vec<Span>) -> zgui::view::AnyView {
    use crate::Erase;

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
            Span::Strike(inside) => view! {
                text(class = "md__strike") {{spans_view(inside)}}
            }
            .any(),
            Span::Link { text, .. } => view! {
                text(class = "md__link") {{spans_view(text)}}
            }
            .any(),
            Span::Image { alt, src, width } => image_view(&alt, &src, width, false),
            Span::Kbd(inside) => view! {
                text(class = "md__kbd") {{spans_view(inside)}}
            }
            .any(),
            Span::Break => view! { text {{"\n".to_owned()}} }.any(),
        })
        .collect::<Vec<_>>()
        .any()
}

/// A fenced block, highlighted as whatever it says it is.
///
/// An editor, and not styled text. A signature is the most valuable line in a hover, and showing
/// it in one colour throws away what makes it readable. Never focused, so never typed into. It is
/// read-only without a flag, exactly as the picker's preview is.
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

    // The language is set through the handle, because the prop takes a name and a fence that
    // named nothing answers "none".
    let named = language.clone();
    let on_ready = Box::new(move |ready: zgui_editor::EditorHandle| {
        ready.set_language(named.as_deref());
        handle.set(Some(ready));
    }) as Box<dyn Fn(zgui_editor::EditorHandle)>;

    view! {
        box(class = "md__block") {
            Editor(
                class = "md__editor",
                text = text,
                autofocus = false,
                config = code_config(lines),
                on_ready = on_ready,
            )
        }
    }
}

/// How a fenced block behaves: no gutter, no caret, no animation, nothing to interact with.
///
/// A line window over every line, so the editor sizes itself to exactly its content and never
/// scrolls. A height stated in CSS drifts from the editor's own rounded line height at
/// fractional scales, and the drift is a few pixels of scroll.
fn code_config(lines: usize) -> zgui_editor::EditorConfig {
    zgui_editor::EditorConfig {
        gutter: zgui_editor::GutterMode::None,
        blink: false,
        smooth_scroll: false,
        line_window: Some(0..lines),
        ..zgui_editor::EditorConfig::default()
    }
}
