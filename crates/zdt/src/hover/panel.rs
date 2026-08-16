//! The panel the documentation is drawn in.

use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};
use zgui::{component, view};
use zgui_ui_primitives::popper::{Align, Placement, Side};
use zgui_ui_primitives::prelude::*;

use super::{Showing, css, device, use_hover};
use crate::markdown::MarkdownProps;
use zdt_view::anchor::{Anchoring, place};

#[component]
pub fn HoverPanel() -> impl IntoView {
    let hover = use_hover();
    let surface = NodeRef::new();

    // What it was showing, kept for the length of the exit: the documentation is cleared the
    // moment the panel closes, and a panel that read it directly would empty out as it left.
    let showing: RwSignal<Option<Showing>, LocalStorage> = RwSignal::new_local(None);
    let follow = zgui::reactive::RenderEffect::new(move |_| {
        if let Some(what) = hover.showing() {
            showing.set(Some(what));
        }
    });
    on_cleanup_local(move || drop(follow));

    view! {
        Presence(
            present = Signal::derive_local(move || hover.showing().is_some()),
            surface = surface
        ) {
            // The panel is built once for as long as it is up, and the documentation inside it
            // changes. Rebuilding the whole panel whenever the text changed, which is what
            // pressing `K` on a second symbol does, built a new one against a handle that still
            // named the *previous* panel's element. That element has already left the document by
            // then. Observing it resolves a node that is gone, which is a panic, and the editor
            // went with it. It is also plain waste: a fenced block mounts an editor of its own,
            // and every one of them was thrown away and made again to change the words above
            // it.
            Panel(showing = showing, surface = surface)
        }
    }
}

/// One panel of documentation.
#[component]
pub(crate) fn Panel(
    /// What to show, and where. Read reactively, so a second `K` changes the words in this panel
    /// and leaves the panel standing.
    showing: RwSignal<Option<Showing>, LocalStorage>,
    /// The panel itself, whose exit animation says when it may be taken away.
    surface: NodeRef,
) -> impl IntoView {
    let hover = use_hover();
    let leaving = use_presence();
    let body = NodeRef::new();

    // The presence's handle is shared with every panel before this one and nothing clears it when
    // an element goes away, so it still names one that has left the document. Cleared before
    // anything observes it; the element below binds it again.
    surface.unbind();

    // Bottom-left corner on the caret, so the documentation grows up and to the right and leaves
    // the line it is about uncovered. The solver flips it below and slides it left at the edges.
    let placed = place(
        surface,
        move || showing.get().map(|what| what.caret),
        Anchoring::on(Placement::new(Side::Top, Align::Start)),
    );

    // How far it can be scrolled, asked of the container. Observed once, here. Asking inside the
    // effect would start a fresh observation on every run.
    let position = body.observe_scroll();
    let watching = zgui::reactive::RenderEffect::new(move |_| {
        let at = position.get();
        // The container answers in device pixels; the keys count in CSS ones.
        hover.set_limit(css(
            surface.scale(),
            at.content_size.height.0 - at.scrollport.height.0,
        ));
    });
    on_cleanup_local(move || drop(watching));

    // The offset is asked for, and never applied here, so the engine owns the movement and
    // re-fragments what it moves. `Smooth` is the same glide the rest of the window scrolls with.
    let scrolling = zgui::reactive::RenderEffect::new(move |_| {
        let offset = device(surface.scale(), hover.offset());
        body.scroll_to(
            ScrollTarget::Offset(zgui::geom::Point::new(
                zgui::geom::DevicePx(0.0),
                zgui::geom::DevicePx(offset),
            )),
            ScrollBehavior::Smooth,
        );
    });
    on_cleanup_local(move || drop(scrolling));

    view! {
        column(
            class = "hover",
            node_ref = surface,
            attr:data-state = move || zdt_view::leaving_state(leaving),
            attr:data-side = move || placed.side.get(),
            attr:data-focused = move || hover.focused.get().then(|| "true".to_owned()),
            style:left = placed.left_px(),
            style:top = placed.top_px(),
            style:visibility = placed.visibility(),
            a11y:role = Role::Tooltip,
            a11y:label = "Documentation"
        ) {
            // Driven by the keys. The style sheet takes the bar away.
            scroll(class = "hover__body", node_ref = body) {
                {move || {
                    use zdt_view::Erase;
                    match showing.get() {
                        Some(what) => view! { Markdown(blocks = what.blocks) }.any(),
                        None => ().any(),
                    }
                }}
            }
        }
    }
}
