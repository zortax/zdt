//! The fill and stroke of the selection: swatches, and a stroke width stepper.

use zgui::prelude::*;
use zgui::{component, view};

use super::super::model::write::fmt;
use super::super::{SvgState, commit};
use crate::workspace::{BufferId, use_workspace};

/// The palette. `none` clears the paint.
const SWATCHES: &[&str] = &[
    "none", "#000000", "#ffffff", "#64748b", "#e11d48", "#f97316", "#eab308", "#22c55e", "#0ea5e9",
    "#3b82f6", "#8b5cf6", "#ec4899",
];

/// How far one press moves the stroke width.
const WIDTH_STEP: f64 = 0.5;

#[component]
pub fn PaintPanel(
    /// Which buffer the edits land in.
    buffer: BufferId,
    /// The preview's state.
    state: SvgState,
) -> impl IntoView {
    use zdt_view::Erase;

    let workspace = use_workspace();

    // What the selection holds now. Tracked, so a commit redraws the lit swatch.
    let selection = move || {
        let at = state.selected.get()?;
        state.model.with(|model| {
            let node = model.as_ref()?.node(at)?;
            Some((
                node.fill.clone(),
                node.stroke.clone(),
                node.stroke_width,
                node.styled,
            ))
        })
    };

    let set = {
        let workspace = workspace.clone();
        move |name: &'static str, value: String| {
            let Some(at) = state.selected.get_untracked() else {
                return;
            };
            let edit = state.model.with_untracked(|model| {
                model
                    .as_ref()
                    .and_then(|held| held.set_attr(at, name, &value))
            });
            if let Some(edit) = edit {
                commit(&workspace, buffer, state, edit);
            }
        }
    };

    let row = {
        let set = set.clone();
        move |name: &'static str, current: Option<String>| {
            let mut faces: Vec<AnyView> = Vec::new();
            for &swatch in SWATCHES {
                let lit = match (&current, swatch) {
                    (Some(held), _) => held.eq_ignore_ascii_case(swatch),
                    // No attribute written: SVG fills black and strokes nothing.
                    (None, "#000000") => name == "fill",
                    (None, "none") => name == "stroke",
                    _ => false,
                };
                let set = set.clone();
                faces.push(
                    view! {
                        control(
                            class = "svgpaint__swatch",
                            tabindex = Focus::Programmatic,
                            a11y:label = format!("{name} {swatch}"),
                            attr:data-none = (swatch == "none").then(|| "true".to_owned()),
                            attr:data-on = lit.then(|| "true".to_owned()),
                            style:background = (swatch != "none").then(|| swatch.to_owned()),
                            on:pointer_down = move |ev: &mut EventCx<'_, events::PointerDown>| {
                                set(name, swatch.to_owned());
                                ev.stop_propagation();
                            }
                        ) {}
                    }
                    .any(),
                );
            }
            faces
        }
    };

    let body = move || {
        let Some((fill, stroke, width, styled)) = selection() else {
            return view! {
                label(class = "svgpaint__hint") {"Select an element to paint it"}
            }
            .any();
        };
        if styled {
            return view! {
                label(class = "svgpaint__hint") {"This element's style attribute wins; edit it in the source"}
            }
            .any();
        }
        let fill_row = row("fill", fill);
        let stroke_row = row("stroke", stroke);
        let thinner = {
            let set = set.clone();
            move |_: &mut EventCx<'_, events::PointerDown>| {
                set("stroke-width", fmt((width - WIDTH_STEP).max(0.0)));
            }
        };
        let thicker = {
            let set = set.clone();
            move |_: &mut EventCx<'_, events::PointerDown>| {
                set("stroke-width", fmt(width + WIDTH_STEP));
            }
        };
        view! {
            column(class = "svgpaint__body") {
                label(class = "svgpaint__title") {"Fill"}
                row(class = "svgpaint__row") { {fill_row} }
                label(class = "svgpaint__title") {"Stroke"}
                row(class = "svgpaint__row") { {stroke_row} }
                row(class = "svgpaint__row") {
                    control(
                        class = "svgpaint__step",
                        tabindex = Focus::Programmatic,
                        a11y:label = "Thinner stroke",
                        on:pointer_down = thinner
                    ) { label {"−"} }
                    label(class = "svgpaint__width") { {format!("{} px", fmt(width))} }
                    control(
                        class = "svgpaint__step",
                        tabindex = Focus::Programmatic,
                        a11y:label = "Thicker stroke",
                        on:pointer_down = thicker
                    ) { label {"+"} }
                }
            }
        }
        .any()
    };

    view! {
        column(class = "svgpaint", a11y:role = Role::Group, a11y:label = "Fill and stroke") {
            {move || body()}
        }
    }
    .any()
}
