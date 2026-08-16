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
    use zdt_view::Erase;

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
                            // A mark in its own box, so a wrapped item's second line lines up
                            // with its first.
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
    use zdt_view::Erase;

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
