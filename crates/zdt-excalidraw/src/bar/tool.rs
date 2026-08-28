//! The tool row.
//!
//! The tools first, then the view, then undo and redo — the order a hand reaches for them in.

use zgui::prelude::*;
use zgui::{component, view};

use crate::state::{Board, Tool};

use super::{Face, ToolbarProps};

/// Every tool, with the outline and the name it is shown by.
const TOOLS: &[(Tool, &str, &str)] = &[
    (Tool::Select, zdt_icons::MOUSE_POINTER, "Select"),
    (Tool::Hand, zdt_icons::HAND, "Pan"),
    (Tool::Rectangle, zdt_icons::SQUARE, "Rectangle"),
    (Tool::Diamond, zdt_icons::DIAMOND, "Diamond"),
    (Tool::Ellipse, zdt_icons::CIRCLE, "Ellipse"),
    (Tool::Arrow, zdt_icons::MOVE_RIGHT, "Arrow"),
    (Tool::Line, zdt_icons::MINUS, "Line"),
    (Tool::Freedraw, zdt_icons::PENCIL, "Draw"),
    (Tool::Text, zdt_icons::TYPE, "Text"),
    (Tool::Image, zdt_icons::IMAGE, "Image"),
    (Tool::Frame, zdt_icons::FRAME, "Frame"),
    (Tool::Eraser, zdt_icons::ERASER, "Eraser"),
];

/// The row of tools and view controls.
#[component]
pub fn ToolRow(
    /// The editor this belongs to.
    board: Board,
) -> impl IntoView {
    let mut tools: Vec<Face> = TOOLS
        .iter()
        .map(|(tool, icon, label)| {
            let tool = *tool;
            Face::toggle(
                icon,
                label,
                move || board.tool.get() == tool,
                move || board.tool.set(tool),
            )
        })
        .collect();

    tools.push(Face::divider());
    tools.push(Face::toggle(
        zdt_icons::SLIDERS,
        "Style",
        move || board.panel.get(),
        move || board.panel.update(|held| *held = !*held),
    ));
    tools.push(Face::divider());
    tools.push(Face::action(zdt_icons::ZOOM_OUT, "Zoom out", move || {
        board.viewport.zoom_by(1.0 / crate::viewport::STEP, None);
    }));
    tools.push(Face::action(zdt_icons::ZOOM_IN, "Zoom in", move || {
        board.viewport.zoom_by(crate::viewport::STEP, None);
    }));
    tools.push(Face::action(
        zdt_icons::MAXIMIZE,
        "Fit the drawing",
        move || {
            crate::actions::fit(&board);
        },
    ));

    view! { Toolbar(tools = tools, label = "Drawing tools") }
}
