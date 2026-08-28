//! The drawing, as the document that shows it.
//!
//! One element per band, in the drawing's own order, so what is drawn last is on top whatever kind
//! it is. Every band is placed over the whole view and takes no pointer events: the surface above
//! them takes all of those.

use zgui::prelude::*;
use zgui::{component, view};

use crate::layers::{band, images, shapes, text};
use crate::state::Board;

/// The drawing.
///
/// One element per band, keyed by where the band begins and what it is. A band that has not changed
/// keeps the element it was drawn into, so an edit rebuilds only what it touched — and moving the
/// view rebuilds nothing at all, because the bands do not depend on what is on screen.
#[component]
pub fn Layers(
    /// The editor this belongs to.
    board: Board,
    /// The pictures it draws.
    pictures: images::Pictures,
) -> impl IntoView {
    view! {
        box(class = "exdraw__layers") {
            for band in move || band::of(board.read().elements()), key = |band: &band::Band| (band.kind(), band.at()) {
                Band(board = board, at = band.at(), pictures = pictures.clone())
            }
        }
    }
}

/// One band, whichever kind it turns out to be.
///
/// Which kind is decided again here rather than passed in, so a band whose run grew or shrank keeps
/// the element it was drawn into.
#[component]
fn Band(
    /// The editor this belongs to.
    board: Board,
    /// Where the band begins.
    at: usize,
    /// The pictures the drawing draws.
    pictures: images::Pictures,
) -> impl IntoView {
    use zdt_view::Erase;

    let kind = band::of(board.read_untracked().elements())
        .into_iter()
        .find(|band| band.at() == at);

    match kind {
        Some(band::Band::Text(at)) => view! { TextBand(board = board, at = at) }.any(),
        Some(band::Band::Image(at)) => {
            view! { ImageBand(board = board, at = at, pictures = pictures) }.any()
        }
        Some(band::Band::Shapes(_)) => view! { ShapeBand(board = board, from = at) }.any(),
        None => ().any(),
    }
}

/// One run of shapes, in one canvas.
#[component]
fn ShapeBand(
    /// The editor this belongs to.
    board: Board,
    /// Where the run begins.
    from: usize,
) -> impl IntoView {
    // The shapes are pushed in the scene's own coordinates and the view box says which part of the
    // scene is shown, so a pan writes the box and re-runs nothing here.
    let view_box = move || {
        let [x, y, width, height] = board.viewport.view_box();
        zgui::vocab::PropValue::from(format!("{x} {y} {width} {height}").as_str())
    };

    let drawn = zgui::elements::canvas()
        .class("exdraw__band")
        .property(
            zgui::view::PropKey::new(zgui::vocab::prop::drawing::VIEW_BOX),
            view_box,
        )
        .draw(move |cx| {
            let scene = board.read();
            // Where the run ends is looked up rather than held, so a shape added to the end of it
            // is drawn without the band being built again.
            let Some(band::Band::Shapes(range)) = band::of(scene.elements())
                .into_iter()
                .find(|band| band.at() == from)
            else {
                return;
            };
            // What a drag is holding is drawn where the drag has taken it, in its own band, so it
            // stays in the order it was drawn in.
            //
            // Whether this band holds any of it is asked first, and the drag itself is only read
            // when it does — so a band with nothing selected in it is not drawn again on every
            // movement of the pointer.
            let carried: rustc_hash::FxHashSet<excalidraw::Id> = scene
                .elements()
                .get(range.clone())
                .unwrap_or_default()
                .iter()
                .filter(|element| crate::pointer::is_dragged(&board, &element.id))
                .map(|element| element.id.clone())
                .collect();
            let dragged = (!carried.is_empty())
                .then(|| crate::pointer::drag_transform(&board))
                .flatten();
            let pieces = board.drawn.try_update_value(|cache| {
                shapes::pieces(
                    cache,
                    scene.elements(),
                    range,
                    dragged,
                    |id| carried.contains(id),
                    |id| board.fade(id),
                )
            });
            let Some(pieces) = pieces else {
                return;
            };
            shapes::push(cx.scene, &pieces, board.dark());
        });

    view! { {drawn} }
}

/// One text element.
#[component]
fn TextBand(
    /// The editor this belongs to.
    board: Board,
    /// Which element.
    at: usize,
) -> impl IntoView {
    use zdt_view::Erase;

    let placed = move || {
        let scene = board.read();
        let element = scene.elements().get(at)?;
        // Words being typed are drawn by the editor over them, not here.
        if board.editing.get().as_ref() == Some(&element.id) {
            return None;
        }
        let container = text::container_of(element, scene.elements());
        let dragged = crate::pointer::is_dragged(&board, &element.id)
            .then(|| crate::pointer::drag_transform(&board))
            .flatten();
        let mut placed = text::placed(element, container, &board.viewport, dragged)?;
        placed.color = crate::color::css(&placed.color, board.dark());
        placed.alpha *= board.fade(&element.id);
        Some(placed)
    };

    let px = |value: f64| Some(format!("{value}px"));
    let held = move |take: fn(&text::Placed) -> f64| move || placed().map(|held| take(&held));

    view! {
        {move || {
            let Some(held) = placed() else {
                return ().any();
            };
            let lines: Vec<AnyView> = held
                .text
                .split('\n')
                .map(|line| {
                    // An empty line still takes a line's height, which a zero-width space is what
                    // gives it.
                    let words = if line.is_empty() { "\u{200b}" } else { line };
                    view! { label(class = "exdraw__line") { {words.to_owned()} } }.any()
                })
                .collect();
            let transform = (held.angle.abs() > f64::EPSILON)
                .then(|| format!("rotate({}deg)", held.angle));
            view! {
                column(
                    class = "exdraw__text",
                    style:left = px(held.at.x),
                    style:top = px(held.at.y),
                    style:width = px(held.width),
                    style:height = px(held.height),
                    style:font-size = px(held.font_size),
                    style:line-height = Some(held.line_height.to_string()),
                    style:font-family = Some(text::family_stack(held.family)),
                    style:color = Some(held.color.clone()),
                    style:opacity = Some(held.alpha.to_string()),
                    style:text-align = Some(text::align_word(held.align).to_owned()),
                    style:justify-content = Some(text::vertical_word(held.vertical).to_owned()),
                    style:transform = transform,
                    a11y:hidden = true
                ) {
                    {lines}
                }
            }
            .any()
        }}
        // Read once so the closure above is the only thing that reads them.
        {move || { let _ = held(|held| held.width); ().any() }}
    }
}

/// One picture.
#[component]
fn ImageBand(
    /// The editor this belongs to.
    board: Board,
    /// Which element.
    at: usize,
    /// The pictures the drawing draws.
    pictures: images::Pictures,
) -> impl IntoView {
    use zdt_view::Erase;

    let placed = move || {
        let scene = board.read();
        let element = scene.elements().get(at)?;
        let src = element
            .image()
            .and_then(|held| held.file_id.as_deref())
            .and_then(|id| pictures.src(&scene.drawing.files, id));
        let dragged = crate::pointer::is_dragged(&board, &element.id)
            .then(|| crate::pointer::drag_transform(&board))
            .flatten();
        let mut placed = images::placed(element, src, &board.viewport, dragged)?;
        placed.alpha *= board.fade(&element.id);
        Some(placed)
    };

    view! {
        {move || {
            let Some(held) = placed() else {
                return ().any();
            };
            let px = |value: f64| Some(format!("{value}px"));
            let transform = {
                let held = images::transform(&held);
                (!held.is_empty()).then_some(held)
            };
            let radius = (held.radius > 0.0).then(|| format!("{}px", held.radius));
            // A picture whose bytes are missing is drawn as the box it would have taken, so the
            // drawing still reads and the gap is obvious.
            let inner: AnyView = match held.src.clone() {
                Some(src) => view! { image(class = "exdraw__picture", src = Some(src), a11y:hidden = true) }.any(),
                None => view! { box(class = "exdraw__missing") {} }.any(),
            };
            view! {
                box(
                    class = "exdraw__image",
                    style:left = px(held.at.x),
                    style:top = px(held.at.y),
                    style:width = px(held.width),
                    style:height = px(held.height),
                    style:opacity = Some(held.alpha.to_string()),
                    style:border-radius = radius,
                    style:transform = transform
                ) {
                    {inner}
                }
            }
            .any()
        }}
    }
}
