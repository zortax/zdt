//! One thinking segment, as a row.

use zdt_agent::thread::TimelineItem;
use zdt_icons::{self as icons, IconProps};
use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};
use zgui::{component, view};

use super::glyph::span_text;

/// One thinking segment: a quiet single line, the whole thought a press away.
///
/// The thought itself is never streamed into the timeline. While the segment runs the line shows
/// a spinner and a climbing clock; done, it says how long it took. A press opens the full text.
#[component]
pub(super) fn ThinkRow(
    /// The row's own signal.
    row: RwSignal<TimelineItem, LocalStorage>,
) -> impl IntoView {
    let opened: RwSignal<bool, LocalStorage> = RwSignal::new_local(false);

    let running = move || row.with(|item| !item.done);

    // The clock. The daemon says how long the segment had already run when this view arrived,
    // and a local timer carries it forward, armed only while the segment runs.
    let carried = row.with_untracked(|item| item.elapsed_ms);
    let began = std::time::Instant::now();
    let shown_ms: RwSignal<u64, LocalStorage> = RwSignal::new_local(carried);
    let slot: std::rc::Rc<std::cell::RefCell<Option<zgui::view::time::IntervalHandle>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));
    let ticking = {
        let slot = std::rc::Rc::clone(&slot);
        zgui::reactive::RenderEffect::new(move |_| {
            let on = running();
            *slot.borrow_mut() = (on && zgui::view::time::Timers::current().is_some()).then(|| {
                zgui::view::time::set_interval(std::time::Duration::from_millis(250), move || {
                    shown_ms.set(carried + began.elapsed().as_millis() as u64);
                })
            });
        })
    };
    on_cleanup_local(move || drop((ticking, slot)));

    let glyph = move || {
        if running() {
            icons::LOADER_CIRCLE
        } else {
            icons::LIGHTBULB
        }
    };
    let live = move || running().then(|| "true".to_owned());
    let word = move || {
        row.with(|item| {
            if !item.done {
                "Thinking\u{2026}".to_owned()
            } else if item.elapsed_ms < 1000 {
                "Thought".to_owned()
            } else {
                format!("Thought for {}", span_text(item.elapsed_ms))
            }
        })
    };
    let clock = move || {
        if running() {
            span_text(shown_ms.get())
        } else {
            String::new()
        }
    };

    let full = move || row.with(|item| item.text.clone());
    // A model that keeps its reasoning back leaves nothing to open.
    let text_shown =
        move || (!opened.get() || row.with(|item| item.text.is_empty())).then(|| "none".to_owned());
    let toggle = move |event: &mut EventCx<'_, events::PointerDown>| {
        event.stop_propagation();
        opened.update(|held| *held = !*held);
    };

    view! {
        column(class = "agent-think", attr:data-running = live, on:pointer_down = toggle) {
            row(class = "agent-think__head") {
                Icon(
                    icon = Signal::derive_local(glyph),
                    class = "icon--xs agent-think__glyph"
                )
                label(class = "agent-think__word") {{word}}
                box(class = "fill") {}
                label(class = "agent-think__clock") {{clock}}
            }
            label(class = "agent-think__text", style:display = text_shown) {{full}}
        }
    }
}
