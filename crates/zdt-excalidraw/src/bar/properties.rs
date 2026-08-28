//! The panel that changes how the selection looks.
//!
//! The palette is Excalidraw's own, so a colour chosen here is one the web app offers too. Every
//! control writes one command, and a command that means nothing to what is selected writes nothing.
//!
//! Each row shows what is in use: the first thing selected, or the style a new element takes when
//! nothing is.

use excalidraw::element::style::{FillStyle, Roundness, StrokeStyle};
use excalidraw::scene::{Change, Style};
use excalidraw::{Command, Id};
use zdt_icons::{self as icons, IconProps};
use zgui::prelude::*;
use zgui::{component, view};

use crate::state::Board;

/// The five colours an outline is offered.
const STROKE_PICKS: &[&str] = &["#1e1e1e", "#e03131", "#2f9e44", "#1971c2", "#f08c00"];
/// The five an inside is.
const BACKGROUND_PICKS: &[&str] = &["transparent", "#ffc9c9", "#b2f2bb", "#a5d8ff", "#ffec99"];

/// What one pill shows.
#[derive(Clone, Copy)]
enum Face {
    /// An outline, where one says it better than a word.
    Icon(&'static str),
    /// A word, where no outline does.
    Word(&'static str),
}

/// The panel.
#[component]
pub fn Properties(
    /// The editor this belongs to.
    board: Board,
) -> impl IntoView {
    use zdt_view::Erase;

    // What every control writes. A change with nothing selected changes the style a new element
    // takes instead, so choosing a colour and then drawing gives that colour.
    let write = move |change: Change| {
        let ids: Vec<Id> = board.read_untracked().selection().to_vec();
        if ids.is_empty() {
            board.with_scene(|scene| apply_to_style(&mut scene.style, &change));
            return;
        }
        board.apply(Command::Restyle { ids, change });
    };

    let swatches = move |name: &'static str, picks: &'static [&'static str], stroke: bool| {
        let faces: Vec<AnyView> = picks
            .iter()
            .map(|color| {
                let held = (*color).to_owned();
                let lit = {
                    let held = held.clone();
                    move || {
                        let scene = board.read();
                        let current = scene
                            .selected()
                            .next()
                            .map(|element| {
                                if stroke {
                                    element.stroke_color.clone()
                                } else {
                                    element.background_color.clone()
                                }
                            })
                            .unwrap_or_else(|| {
                                if stroke {
                                    scene.style.stroke_color.clone()
                                } else {
                                    scene.style.background_color.clone()
                                }
                            });
                        current.eq_ignore_ascii_case(&held)
                    }
                };
                let held = held.clone();
                view! {
                    control(
                        class = "exdrawpanel__swatch",
                        tabindex = Focus::Programmatic,
                        a11y:label = format!("{name} {held}"),
                        attr:data-none = (held == "transparent").then(|| "true".to_owned()),
                        attr:data-on = move || lit().then(|| "true".to_owned()),
                        style:background = (held != "transparent").then(|| held.clone()),
                        on:pointer_down = {
                            let held = held.clone();
                            move |ev: &mut EventCx<'_, events::PointerDown>| {
                                write(if stroke {
                                    Change::StrokeColor(held.clone())
                                } else {
                                    Change::BackgroundColor(held.clone())
                                });
                                ev.stop_propagation();
                            }
                        }
                    ) {}
                }
                .any()
            })
            .collect();
        faces
    };

    // One row of pills. Each one is lit when it is the one in use, so the panel says what the
    // selection is as well as what it could be.
    let pills = move |name: &'static str,
                      picks: Vec<(&'static str, Face, Change)>,
                      of_element: fn(&excalidraw::Element) -> Change,
                      of_style: fn(&Style) -> Change| {
        let faces: Vec<AnyView> = picks
            .into_iter()
            .map(|(word, face, change)| {
                let lit = {
                    let change = change.clone();
                    move || chosen(&board, of_element, of_style) == change
                };
                let inside: AnyView = match face {
                    Face::Icon(icon) => view! { Icon(icon = icon, class = "icon--xs") }.any(),
                    Face::Word(held) => {
                        view! { label(class = "nowrap") { {held.to_owned()} } }.any()
                    }
                };
                view! {
                    control(
                        class = "exdrawpill__face",
                        tabindex = Focus::Programmatic,
                        a11y:label = format!("{name} {word}"),
                        attr:data-on = move || lit().then(|| "true".to_owned()),
                        on:pointer_down = move |ev: &mut EventCx<'_, events::PointerDown>| {
                            write(change.clone());
                            ev.stop_propagation();
                        }
                    ) { {inside} }
                }
                .any()
            })
            .collect();
        faces
    };

    let fills = pills(
        "Fill",
        vec![
            (
                "Solid",
                Face::Icon(icons::SQUARE),
                Change::FillStyle(FillStyle::Solid),
            ),
            (
                "Hachure",
                Face::Icon(icons::TEXT_ALIGN_JUSTIFY),
                Change::FillStyle(FillStyle::Hachure),
            ),
            (
                "Cross-hatch",
                Face::Icon(icons::HASH),
                Change::FillStyle(FillStyle::CrossHatch),
            ),
            (
                "Zigzag",
                Face::Icon(icons::WAVES),
                Change::FillStyle(FillStyle::ZigZag),
            ),
        ],
        |element| Change::FillStyle(element.fill_style),
        |style| Change::FillStyle(style.fill_style),
    );

    let widths = pills(
        "Stroke width",
        vec![
            ("Thin", Face::Word("S"), Change::StrokeWidth(1.0)),
            ("Medium", Face::Word("M"), Change::StrokeWidth(2.0)),
            ("Bold", Face::Word("L"), Change::StrokeWidth(4.0)),
        ],
        |element| Change::StrokeWidth(element.stroke_width),
        |style| Change::StrokeWidth(style.stroke_width),
    );

    let strokes = pills(
        "Stroke style",
        vec![
            (
                "Solid",
                Face::Icon(icons::MINUS),
                Change::StrokeStyle(StrokeStyle::Solid),
            ),
            (
                "Dashed",
                Face::Icon(icons::CHART_GANTT),
                Change::StrokeStyle(StrokeStyle::Dashed),
            ),
            (
                "Dotted",
                Face::Icon(icons::ELLIPSIS),
                Change::StrokeStyle(StrokeStyle::Dotted),
            ),
        ],
        |element| Change::StrokeStyle(element.stroke_style),
        |style| Change::StrokeStyle(style.stroke_style),
    );

    let sloppiness = pills(
        "Sloppiness",
        vec![
            (
                "Architect",
                Face::Icon(icons::RULER),
                Change::Roughness(0.0),
            ),
            ("Artist", Face::Icon(icons::PENCIL), Change::Roughness(1.0)),
            (
                "Cartoonist",
                Face::Icon(icons::SIGNATURE),
                Change::Roughness(2.0),
            ),
        ],
        |element| Change::Roughness(element.roughness),
        |style| Change::Roughness(style.roughness),
    );

    let corners = pills(
        "Corners",
        vec![
            ("Sharp", Face::Word("Sharp"), Change::Roundness(None)),
            ("Round", Face::Word("Round"), Change::Roundness(rounded())),
        ],
        // Any roundness at all reads as round: the panel offers one kind, and a file may hold
        // another.
        |element| Change::Roundness(element.roundness.is_some().then(rounded).flatten()),
        |style| Change::Roundness(style.roundness.then(rounded).flatten()),
    );

    let opacities = pills(
        "Opacity",
        vec![
            ("Faint", Face::Word("30"), Change::Opacity(30.0)),
            ("Half", Face::Word("60"), Change::Opacity(60.0)),
            ("Solid", Face::Word("100"), Change::Opacity(100.0)),
        ],
        |element| Change::Opacity(element.opacity),
        |style| Change::Opacity(style.opacity),
    );

    let arrange: Vec<AnyView> = [
        (icons::SEND_TO_BACK, "Send to back", Arrange::Back),
        (icons::BRING_TO_FRONT, "Bring to front", Arrange::Front),
        (icons::GROUP, "Group", Arrange::Group),
        (icons::UNGROUP, "Ungroup", Arrange::Ungroup),
        (icons::TRASH, "Delete", Arrange::Delete),
    ]
    .into_iter()
    .map(|(icon, label, what)| {
        view! {
            control(
                class = "exdrawpanel__act",
                tabindex = Focus::Programmatic,
                a11y:label = label,
                on:pointer_down = move |ev: &mut EventCx<'_, events::PointerDown>| {
                    arranged(&board, what);
                    ev.stop_propagation();
                }
            ) {
                Icon(icon = icon, class = "icon--xs")
            }
        }
        .any()
    })
    .collect();

    view! {
        column(class = "exdrawpanel", a11y:role = Role::Group, a11y:label = "Drawing style") {
            Title(icon = icons::PALETTE, word = "Stroke")
            row(class = "exdrawpanel__row") { {swatches("Stroke", STROKE_PICKS, true)} }
            Title(icon = icons::PAINT_BUCKET, word = "Background")
            row(class = "exdrawpanel__row") { {swatches("Background", BACKGROUND_PICKS, false)} }
            Title(icon = icons::BLEND, word = "Fill")
            row(class = "exdrawpill", a11y:role = Role::Group) { {fills} }
            Title(icon = icons::PEN_LINE, word = "Stroke width")
            row(class = "exdrawpill", a11y:role = Role::Group) { {widths} }
            Title(icon = icons::SQUARE_DASHED, word = "Stroke style")
            row(class = "exdrawpill", a11y:role = Role::Group) { {strokes} }
            Title(icon = icons::SPLINE, word = "Sloppiness")
            row(class = "exdrawpill", a11y:role = Role::Group) { {sloppiness} }
            Title(icon = icons::SQUIRCLE, word = "Corners")
            row(class = "exdrawpill", a11y:role = Role::Group) { {corners} }
            Title(icon = icons::CONTRAST, word = "Opacity")
            row(class = "exdrawpill", a11y:role = Role::Group) { {opacities} }
            Title(icon = icons::LAYERS, word = "Arrange")
            row(class = "exdrawpanel__row") { {arrange} }
        }
    }
}

/// The name of one section, with the outline that stands for it.
#[component]
fn Title(
    /// The outline.
    icon: &'static str,
    /// What the section is called.
    word: &'static str,
) -> impl IntoView {
    view! {
        row(class = "exdrawpanel__title") {
            Icon(icon = icon, class = "icon--xs")
            label { {word.to_owned()} }
        }
    }
}

/// The one kind of roundness the panel offers.
fn rounded() -> Option<Roundness> {
    Some(Roundness::Adaptive { value: None })
}

/// What the panel shows as chosen.
///
/// The first thing selected speaks for the rest: a row shows one value, and the selection may hold
/// several. With nothing selected it is the style a new element takes.
fn chosen(
    board: &Board,
    of_element: fn(&excalidraw::Element) -> Change,
    of_style: fn(&Style) -> Change,
) -> Change {
    let scene = board.read();
    scene
        .selected()
        .next()
        .map_or_else(|| of_style(&scene.style), of_element)
}

/// What one of the arrange buttons does.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Arrange {
    /// Behind everything.
    Back,
    /// In front of it.
    Front,
    /// Into a group.
    Group,
    /// Out of one.
    Ungroup,
    /// Away.
    Delete,
}

/// Does one of them.
fn arranged(board: &Board, what: Arrange) {
    let ids: Vec<Id> = board.read_untracked().selection().to_vec();
    if ids.is_empty() {
        return;
    }
    let command = match what {
        Arrange::Back => Command::Reorder {
            ids,
            order: excalidraw::scene::Order::Back,
        },
        Arrange::Front => Command::Reorder {
            ids,
            order: excalidraw::scene::Order::Front,
        },
        Arrange::Group => Command::Group(ids),
        Arrange::Ungroup => Command::Ungroup(ids),
        Arrange::Delete => Command::Delete(ids),
    };
    board.apply(command);
}

/// The same change, to the style a new element takes.
fn apply_to_style(style: &mut excalidraw::scene::Style, change: &Change) {
    match change {
        Change::StrokeColor(color) => style.stroke_color = color.clone(),
        Change::BackgroundColor(color) => style.background_color = color.clone(),
        Change::FillStyle(held) => style.fill_style = *held,
        Change::StrokeWidth(held) => style.stroke_width = *held,
        Change::StrokeStyle(held) => style.stroke_style = *held,
        Change::Roughness(held) => style.roughness = *held,
        Change::Opacity(held) => style.opacity = *held,
        Change::Roundness(held) => style.roundness = held.is_some(),
        Change::FontSize(held) => style.font_size = *held,
        Change::FontFamily(held) => style.font_family = *held,
        Change::TextAlign(held) => style.text_align = *held,
        Change::VerticalAlign(held) => style.vertical_align = *held,
        Change::StartArrowhead(held) => style.start_arrowhead = *held,
        Change::EndArrowhead(held) => style.end_arrowhead = *held,
        // Neither is something a new element is given.
        Change::Locked(_) | Change::Link(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A row shows the value of what is selected, and the style a new element takes when nothing
    /// is.
    #[test]
    fn a_row_shows_what_is_in_use() {
        let window = zgui_testkit_view::Window::open();
        window.scope.with(|| {
            let text = r#"{"type":"excalidraw","elements":[
                {"type":"rectangle","id":"a","x":0,"y":0,"width":10,"height":10,"roughness":0}]}"#;
            let drawing = excalidraw::file::parse(text).expect("a drawing");
            let board = Board::new(excalidraw::Scene::new(drawing, 1, 1));
            let roughness = |board: &Board| {
                chosen(
                    board,
                    |element| Change::Roughness(element.roughness),
                    |style| Change::Roughness(style.roughness),
                )
            };

            let style = board.read_untracked().style.roughness;
            assert_eq!(roughness(&board), Change::Roughness(style), "the next one");

            board.with_scene(|scene| scene.select([excalidraw::Id::new("a")]));
            assert_eq!(roughness(&board), Change::Roughness(0.0), "the chosen one");
        });
    }
}
