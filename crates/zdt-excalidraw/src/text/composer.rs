//! What is shown while words are being typed.
//!
//! Not a field: the keyboard belongs to the editor, and a box that had to hold focus would fight
//! whatever put it there. What is typed is kept on the board and this only shows it, over the
//! element it belongs to and at the size it will be drawn.

use zgui::prelude::*;
use zgui::{component, view};

use crate::layers::text;
use crate::state::Board;

/// The words being typed.
#[component]
pub fn Composer(
    /// The editor this belongs to.
    board: Board,
) -> impl IntoView {
    use zdt_view::Erase;

    view! {
        {move || {
            let Some(id) = board.editing.get() else {
                return ().any();
            };
            let scene = board.read();
            let Some(element) = scene.element(&id).cloned() else {
                return ().any();
            };
            let container = text::container_of(&element, scene.elements()).cloned();
            drop(scene);
            let Some(placed) = text::placed(&element, container.as_ref(), &board.viewport, None) else {
                return ().any();
            };

            let typed = board.typing.get();
            let px = |value: f64| Some(format!("{value}px"));
            let transform = (placed.angle.abs() > f64::EPSILON)
                .then(|| format!("rotate({}deg)", placed.angle));
            // A caret at the end, so an empty box still shows where the words will go.
            let lines: Vec<AnyView> = {
                let shown = format!("{typed}\u{2502}");
                shown
                    .split('\n')
                    .map(|line| view! { label(class = "exdraw__line") { {line.to_owned()} } }.any())
                    .collect()
            };

            view! {
                column(
                    class = "exdraw__composer",
                    style:left = px(placed.at.x),
                    style:top = px(placed.at.y),
                    style:min-width = px(placed.font_size),
                    style:font-size = px(placed.font_size),
                    style:line-height = Some(placed.line_height.to_string()),
                    style:font-family = Some(text::family_stack(placed.family)),
                    style:color = Some(crate::color::css(&placed.color, board.dark())),
                    style:text-align = Some(text::align_word(placed.align).to_owned()),
                    style:transform = transform,
                    a11y:role = Role::TextInput,
                    a11y:label = "Words"
                ) {
                    {lines}
                }
            }
            .any()
        }}
    }
}
