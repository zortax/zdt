//! Renaming a symbol, over the symbol.
//!
//! A one-line box where the word is, rather than a prompt in the middle of the window. The
//! difference matters: a rename is about a particular word, and a box somewhere else makes
//! somebody hold that word in their head while they type its replacement.
//!
//! # Why it is an overlay rather than an inline edit
//!
//! The editor cannot host a widget inside the text — there is no virtual text, no inline widget,
//! and every line is exactly one line high. So the box floats over the symbol and the symbol is
//! banded underneath it in its own decoration layer, which is what says *this* is the thing being
//! renamed. That band is not decoration: without it a box that has flipped above a symbol near the
//! bottom of the window is a box with no visible connection to anything.

use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};
use zgui::{component, view};
use zgui_ui::prelude::*;
use zgui_ui_primitives::prelude::*;

use crate::ui::anchor::{Anchoring, place};

/// The layer the symbol being renamed is banded in.
///
/// Its own, beside the diagnostics' and the git signs', so that opening the box does not clear
/// either and closing it does not have to put them back.
const LAYER: &str = "rename";

/// What is being renamed.
#[derive(Clone, PartialEq, Debug)]
pub struct Asking {
    /// What it is called now, which is what the box opens holding.
    pub name: String,
    /// Which bytes of the buffer it is.
    pub range: std::ops::Range<usize>,
    /// Where that is on the window.
    pub caret: zgui_editor::CaretRect,
}

/// The rename box.
#[derive(Clone, Copy)]
pub struct Rename {
    asking: RwSignal<Option<Asking>, LocalStorage>,
}

impl Rename {
    /// Nothing being renamed.
    #[must_use]
    pub fn new() -> Self {
        Self {
            asking: RwSignal::new_local(None),
        }
    }

    /// Opens the box over `range`, holding `name`.
    pub fn open(&self, name: &str, range: std::ops::Range<usize>, caret: zgui_editor::CaretRect) {
        self.asking.set(Some(Asking {
            name: name.to_owned(),
            range,
            caret,
        }));
    }

    /// Puts it away, leaving the buffer as it was.
    pub fn cancel(&self) {
        if self.asking.with_untracked(Option::is_some) {
            self.asking.set(None);
        }
    }

    /// Whether it is open, without subscribing.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.asking.with_untracked(Option::is_some)
    }

    /// What is being renamed. Tracked.
    #[must_use]
    pub fn asking(&self) -> Option<Asking> {
        self.asking.get()
    }

    /// Renames to `to` and closes.
    ///
    /// A name that has not changed is a rename nobody asked for, and running one would be a
    /// round trip and a "changed 9 files" for pressing `<CR>` by accident.
    pub fn submit(&self, workspace: &crate::workspace::Workspace, to: &str) {
        let was = self.asking.get_untracked();
        self.cancel();
        let to = to.trim();
        match was {
            Some(was) if to.is_empty() || to == was.name => {}
            Some(_) => crate::actions::lsp::rename_to(workspace, to),
            None => {}
        }
    }
}

impl Default for Rename {
    fn default() -> Self {
        Self::new()
    }
}

/// Puts the rename box where every component can find it.
pub fn provide(rename: Rename) {
    zgui::reactive::provide_local_context(rename);
}

/// It, from inside a component.
///
/// # Panics
///
/// If none was provided above this component, which is a wiring mistake.
#[must_use]
pub fn use_rename() -> Rename {
    zgui::reactive::use_local_context::<Rename>().expect("a rename box is provided at the root")
}

/// The box.
#[component]
pub fn RenameBox() -> impl IntoView {
    let rename = use_rename();
    let surface = NodeRef::new();

    // What it was asking, kept for the length of the exit.
    let showing: RwSignal<Option<Asking>, LocalStorage> = RwSignal::new_local(None);
    let follow = zgui::reactive::RenderEffect::new(move |_| {
        if let Some(what) = rename.asking() {
            showing.set(Some(what));
        }
    });
    on_cleanup_local(move || drop(follow));

    // The band under the symbol, put on while the box is open and taken off when it closes.
    let workspace = crate::workspace::use_workspace();
    let banding = zgui::reactive::RenderEffect::new(move |_| {
        let Some(handle) = workspace.current_handle() else {
            return;
        };
        match rename.asking() {
            Some(what) => handle.set_decorations(
                LAYER,
                vec![zgui_editor::decoration::Decoration {
                    range: what.range,
                    kind: zgui_editor::decoration::DecorationKind::Background(
                        zgui_editor::decoration::Paint::Property("zdt-match".into()),
                    ),
                }],
            ),
            None => handle.clear_decorations(LAYER),
        }
    });
    on_cleanup_local(move || drop(banding));

    view! {
        Presence(
            present = Signal::derive_local(move || rename.asking().is_some()),
            surface = surface
        ) {
            {move || {
                use crate::ui::Erase;
                match showing.get() {
                    Some(what) => view! { Box(asking = what, surface = surface) }.any(),
                    None => ().any(),
                }
            }}
        }
    }
}

/// One box, rebuilt whenever a different symbol is renamed.
///
/// Built fresh rather than updated, so the field starts out holding the right text: an input's
/// value is written into the element, and rebuilding is the plainest way to write a new one.
#[component]
fn Box(
    /// What is being renamed.
    asking: Asking,
    /// The box itself, whose exit animation says when it may be taken away.
    surface: NodeRef,
) -> impl IntoView {
    let rename = use_rename();
    let workspace = crate::workspace::use_workspace();
    let leaving = use_presence();
    let node = NodeRef::new();
    let value = RwSignal::new_local(asking.name.clone());

    // The presence's handle is shared with every box before this one and nothing clears it when an
    // element goes away. Cleared before anything observes it, as `ui::hover::Panel` does.
    surface.unbind();

    let caret = asking.caret;
    let placed = place(
        surface,
        move || Some(caret),
        Anchoring::default().offset(4.0),
    );

    // From a timer for the same reason the prompt's is: nothing unmounted takes focus.
    let claim = zgui::view::time::Timers::current()
        .map(|timers| timers.set_timeout(std::time::Duration::ZERO, move || node.focus()));
    on_cleanup_local(move || drop(claim));

    let on_key = move |event: &mut EventCx<'_, events::KeyDown>| {
        match &event.key {
            Key::Named(NamedKey::Enter) => {
                rename.submit(&workspace, &value.get_untracked());
                event.prevent_default();
                event.stop_propagation();
            }
            Key::Named(NamedKey::Escape) => {
                rename.cancel();
                event.prevent_default();
                event.stop_propagation();
            }
            // Everything else is text going into the field, and must not reach the editor behind.
            _ => event.stop_propagation(),
        }
    };

    view! {
        row(
            class = "rename",
            node_ref = surface,
            attr:data-state = move || crate::ui::leaving_state(leaving),
            attr:data-side = move || placed.side.get(),
            style:left = placed.left_px(),
            style:top = placed.top_px(),
            style:visibility = placed.visibility(),
            a11y:role = Role::Dialog,
            a11y:label = "Rename"
        ) {
            Input(
                class = "rename__input",
                node_ref = node,
                value = Binding::from(value),
                a11y:label = "New name",
                on:key_down = on_key
            )
        }
    }
}
