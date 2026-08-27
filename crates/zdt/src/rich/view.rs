//! The markdown preview: the buffer's text, parsed and drawn as a document.
//!
//! Mounted beside the editor and shown in its place while the split is in rich form. It stays
//! mounted across a toggle back, so its parse and its reading position are warm the way a hidden
//! editor's are.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RenderEffect, RwSignal};
use zgui::view::time::Timers;
use zgui::{component, view};

use super::{Reading, css, device, use_previews};
use crate::markdown::{Block, MarkdownProps, parse};
use crate::vim::use_vim;
use crate::workspace::{BufferId, WindowId, use_workspace};

/// How long typing settles before the page is parsed again.
const REPARSE_DEBOUNCE: Duration = Duration::from_millis(300);

#[component]
pub fn MarkdownPreview(
    /// Which window it is in.
    window: WindowId,
    /// Which buffer it draws.
    buffer: BufferId,
) -> impl IntoView {
    use zdt_view::Erase;

    let workspace = use_workspace();
    let Some(entry) = workspace.buffer_untracked(buffer) else {
        // The buffer closed between the toggle and this mounting. Nothing to show.
        return view! { box() }.any();
    };
    let Some(document) = entry.document().cloned() else {
        return view! { box() }.any();
    };

    // Where the document's relative image paths point: beside the file, when there is one.
    if let Some(base) = entry.path.as_ref().and_then(|path| path.parent()) {
        zdt_view::markdown::provide_base(zdt_view::markdown::Base(base.to_path_buf()));
    }

    // Parsed once here, and again after every settled edit below.
    let blocks: RwSignal<Vec<Block>, LocalStorage> =
        RwSignal::new_local(parse(&document.rope().to_string()));

    // Where the keyboard lands while the split is in rich form. The projector prefers this sink
    // over the hidden editor's handle exactly while the model says the split is rich.
    let node = NodeRef::new();
    crate::focus::claim::sink(
        crate::focus::Spot::Buffer(window, buffer),
        crate::focus::Sink::Node(node),
    );

    // The reading position, filed so the region's keys can reach it.
    let previews = use_previews();
    let reading = Reading::new();
    previews.register(window, buffer, reading);
    {
        let previews = previews.clone();
        on_cleanup_local(move || previews.forget(window, buffer, reading));
    }

    // Parsing again: after a pause once the text moves, and only while the page is on screen. An
    // edit made while the split shows the source marks the page stale, and showing it parses
    // once. Replacing the timer handle cancels the one before it, which is the debounce.
    let timers = Timers::current();
    let pending: Rc<std::cell::RefCell<Option<zgui::view::time::TimeoutHandle>>> =
        Rc::new(std::cell::RefCell::new(None));
    let stale = Rc::new(Cell::new(false));
    let reparse = {
        let workspace = workspace.clone();
        let document = document.clone();
        let revision = entry.revision;
        let (pending, stale) = (Rc::clone(&pending), Rc::clone(&stale));
        RenderEffect::new(move |previous: Option<(u64, bool)>| {
            let at = revision.get();
            let showing = workspace.is_rich(window, buffer);
            let Some((was_at, _)) = previous else {
                // The mount parse above is this text already.
                return (at, showing);
            };
            let moved = at != was_at;
            if moved && !showing {
                stale.set(true);
            }
            if showing && (moved || stale.get()) {
                stale.set(false);
                if let Some(timers) = timers.as_ref() {
                    let document = document.clone();
                    *pending.borrow_mut() = Some(timers.set_timeout(REPARSE_DEBOUNCE, move || {
                        blocks.set(parse(&document.rope().to_string()));
                    }));
                }
            }
            (at, showing)
        })
    };
    on_cleanup_local(move || drop(reparse));

    // How far it can be scrolled and how tall it stands, asked of the container. Observed once,
    // here: asking inside the effect would start a fresh observation on every run. Left alone
    // while the split shows the source: a hidden container measures nothing, and writing its
    // nothing into the reading would throw the place away on every toggle.
    let body = NodeRef::new();
    let position = body.observe_scroll();
    let watching = {
        let workspace = workspace.clone();
        RenderEffect::new(move |_| {
            let at = position.get();
            if !workspace.is_rich(window, buffer) {
                return;
            }
            // The container answers in device pixels; the keys count in CSS ones.
            reading.set_extent(
                css(
                    node.scale(),
                    at.content_size.height.0 - at.scrollport.height.0,
                ),
                css(node.scale(), at.scrollport.height.0),
            );
        })
    };
    on_cleanup_local(move || drop(watching));

    // The offset is asked for, and never applied here, so the engine owns the movement and
    // re-fragments what it moves. Never asked of a hidden container: a glide it cannot make
    // never ends, and the animation runs the processor for as long as the split shows the
    // source.
    let scrolling = {
        let workspace = workspace.clone();
        RenderEffect::new(move |_| {
            let offset = device(node.scale(), reading.offset());
            if !workspace.is_rich(window, buffer) {
                return;
            }
            body.scroll_to(
                ScrollTarget::Offset(zgui::geom::Point::new(
                    zgui::geom::DevicePx(0.0),
                    zgui::geom::DevicePx(offset),
                )),
                ScrollBehavior::Smooth,
            );
        })
    };
    on_cleanup_local(move || drop(scrolling));

    // The keys. Everything the page answers goes through the region's keymap, with the base map
    // layered underneath, so the toggle and the window keys keep working from in here.
    let vim = use_vim();
    let on_key = move |event: &mut EventCx<'_, events::KeyDown>| {
        if let Some(chord) = crate::keys::chord_of(event, event.modifiers)
            && vim.key_in_region(chord, super::REGION)
        {
            event.prevent_default();
        }
        // Whatever the page did with it, the hidden editor behind must not also see it.
        event.stop_propagation();
    };

    view! {
        column(
            class = "mdpreview",
            node_ref = node,
            tabindex = Focus::Programmatic,
            a11y:role = Role::Document,
            a11y:label = "Markdown preview",
            on:key_down = on_key
        ) {
            scroll(class = "mdpreview__body", node_ref = body) {
                column(class = "mdpreview__page") {
                    {move || {
                        let blocks = blocks.get();
                        view! { Markdown(blocks = blocks) }.any()
                    }}
                }
            }
        }
    }
    .any()
}
