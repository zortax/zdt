//! A parsed document, drawn.

use zgui::prelude::*;
use zgui::{component, view};

use crate::markdown::{Block, Span};

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
