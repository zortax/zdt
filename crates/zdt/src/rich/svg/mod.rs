//! The SVG preview and its editor: the drawing the buffer's text describes, drawn on a stage,
//! with tools that write back into the text.
//!
//! One document, two views. Every edit the tools make is a set of byte replacements applied
//! through the buffer's own history, so `u` in the source view takes a drag back, the dirty
//! mark follows, and `:w` writes what both views show.

mod edit;
pub mod model;
mod render;

use std::cell::RefCell;
use std::rc::Rc;

use rustc_hash::FxHashMap;
use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RenderEffect, RwSignal};
use zgui::{component, view};

use crate::rich::stage::{Camera, StageProps, use_stages};
use crate::rich::toolbar::{Tool as Face, ToolbarProps};
use crate::workspace::{BufferId, WindowId, Workspace, use_workspace};
use edit::SvgEditorProps;
use model::{SvgEdit, SvgModel};

/// The keymap overlay a focused SVG preview answers keys in.
pub const REGION: &str = "svg";

/// Which tool the pointer is.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Tool {
    /// Whole elements: press to select, drag to move, handles to scale.
    #[default]
    Select,
    /// A path's own points: anchors and their control points.
    Nodes,
}

/// One mounted SVG preview's state. Every field is a signal handle, so the whole of it copies.
#[derive(Clone, Copy, PartialEq)]
pub struct SvgState {
    /// The text the vector element draws.
    pub render: RwSignal<String, LocalStorage>,
    /// The snapshot the editor works on.
    pub model: RwSignal<Option<Rc<SvgModel>>, LocalStorage>,
    /// Whether the edit tools are out.
    pub editing: RwSignal<bool, LocalStorage>,
    /// Which tool the pointer is.
    pub tool: RwSignal<Tool, LocalStorage>,
    /// Which node is selected, as an index into the snapshot.
    pub selected: RwSignal<Option<usize>, LocalStorage>,
    /// Whether the paint swatches are out.
    pub painting: RwSignal<bool, LocalStorage>,
    /// The revision of the editor's own last write. The debounce skips it.
    pub expected: RwSignal<Option<u64>, LocalStorage>,
    /// What the corner has to say: an unreadable document, or parts that are not drawn.
    pub notice: RwSignal<Option<String>, LocalStorage>,
}

impl SvgState {
    fn new() -> Self {
        Self {
            render: RwSignal::new_local(String::new()),
            model: RwSignal::new_local(None),
            editing: RwSignal::new_local(false),
            tool: RwSignal::new_local(Tool::Select),
            selected: RwSignal::new_local(None),
            painting: RwSignal::new_local(false),
            expected: RwSignal::new_local(None),
            notice: RwSignal::new_local(None),
        }
    }
}

/// Every mounted SVG preview, by the window and buffer it belongs to.
#[derive(Clone)]
pub struct SvgViews {
    inner: Rc<RefCell<FxHashMap<(WindowId, BufferId), SvgState>>>,
}

impl SvgViews {
    /// Nothing mounted.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Rc::new(RefCell::new(FxHashMap::default())),
        }
    }

    fn register(&self, window: WindowId, buffer: BufferId, state: SvgState) {
        self.inner.borrow_mut().insert((window, buffer), state);
    }

    fn forget(&self, window: WindowId, buffer: BufferId, state: SvgState) {
        let mut held = self.inner.borrow_mut();
        if held.get(&(window, buffer)) == Some(&state) {
            held.remove(&(window, buffer));
        }
    }

    /// The preview the keyboard is in, when it is in one.
    fn current(&self, workspace: &Workspace) -> Option<(BufferId, SvgState)> {
        let window = workspace.focused_untracked();
        let buffer = workspace.buffer_in_untracked(window)?;
        if !workspace.is_rich_untracked(window, buffer) {
            return None;
        }
        let state = self.inner.borrow().get(&(window, buffer)).copied()?;
        Some((buffer, state))
    }
}

impl Default for SvgViews {
    fn default() -> Self {
        Self::new()
    }
}

/// Puts the registry where every component can find it.
pub fn provide(views: SvgViews) {
    zgui::reactive::provide_local_context(views);
}

fn use_views() -> Option<SvgViews> {
    zgui::reactive::use_local_context::<SvgViews>()
}

/// Working the SVG preview under the keyboard, from the keys of its region.
pub fn run(workspace: &Workspace, leaf: &str) {
    let Some(views) = use_views() else {
        return;
    };
    let Some((buffer, state)) = views.current(workspace) else {
        return;
    };

    match leaf {
        "edit" => {
            let on = !state.editing.get_untracked();
            state.editing.set(on);
            if !on {
                state.selected.set(None);
                state.painting.set(false);
            }
        }
        "select_tool" if state.editing.get_untracked() => state.tool.set(Tool::Select),
        "node_tool" if state.editing.get_untracked() => state.tool.set(Tool::Nodes),
        "paint" if state.editing.get_untracked() => {
            state.painting.set(!state.painting.get_untracked());
        }
        "delete" if state.editing.get_untracked() => {
            let Some(at) = state.selected.get_untracked() else {
                return;
            };
            let edit = state
                .model
                .with_untracked(|model| model.as_ref().and_then(|held| held.remove(at)));
            if let Some(edit) = edit {
                state.selected.set(None);
                commit(workspace, buffer, state, edit);
            }
        }
        "escape" => {
            if state.painting.get_untracked() {
                state.painting.set(false);
            } else if state.selected.get_untracked().is_some() {
                state.selected.set(None);
            } else if state.tool.get_untracked() != Tool::Select {
                state.tool.set(Tool::Select);
            } else if state.editing.get_untracked() {
                state.editing.set(false);
            }
        }
        // Silently. The base map layers underneath the region, and an unbound key there falls
        // through to it.
        _ => {}
    }
}

/// Writes `edit` into the buffer's text, through its history, and refreshes the drawing.
///
/// The write goes through [`zgui_editor::Document::apply`], so it is one undo step, every view
/// hears it, and the dirty mark and the session follow through the hidden editor's events.
pub(super) fn commit(workspace: &Workspace, buffer: BufferId, state: SvgState, edit: SvgEdit) {
    let Some(document) = workspace
        .buffer_untracked(buffer)
        .and_then(|entry| entry.document().cloned())
    else {
        return;
    };
    // The ranges address the snapshot's text. A buffer that moved past it gets no edit; the
    // debounce is already parsing the newer text.
    if document.revision() != edit.base {
        return;
    }
    if !document.apply(edit.replacements) {
        return;
    }
    state.expected.set(Some(document.revision()));
    render::refresh(&state, &document);
}

#[component]
pub fn SvgPreview(
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

    // Where the keyboard lands while the split is in rich form.
    let node = NodeRef::new();
    crate::focus::claim::sink(
        crate::focus::Spot::Buffer(window, buffer),
        crate::focus::Sink::Node(node),
    );

    let state = SvgState::new();
    render::refresh(&state, &document);
    let camera = Camera::new();

    let stages = use_stages();
    let views = use_views();
    stages.register(window, buffer, camera);
    if let Some(views) = &views {
        views.register(window, buffer, state);
    }
    on_cleanup_local({
        let stages = stages.clone();
        let views = views.clone();
        move || {
            stages.forget(window, buffer, camera);
            if let Some(views) = &views {
                views.forget(window, buffer, state);
            }
        }
    });

    // The stage wears the document's own space.
    {
        let sizing = RenderEffect::new(move |_| {
            state.model.with(|model| {
                if let Some(model) = model {
                    camera.set_content(model.view_box[2] as f32, model.view_box[3] as f32);
                }
            });
        });
        on_cleanup_local(move || drop(sizing));
    }

    // Typing reaches the drawing after a pause; the editor's own writes are already in it.
    {
        let following =
            render::follow(&workspace, &document, state, window, buffer, entry.revision);
        on_cleanup_local(move || drop(following));
    }

    // The toolbar: the camera tools, and the edit tools while they are out.
    let toolbar = {
        let workspace = workspace.clone();
        move || {
            let mut tools = vec![
                Face::action(zdt_icons::ZOOM_OUT, "Zoom out", move || {
                    camera.zoom_by(1.0 / crate::rich::stage::STEP, None);
                }),
                Face::action(zdt_icons::ZOOM_IN, "Zoom in", move || {
                    camera.zoom_by(crate::rich::stage::STEP, None);
                }),
                Face::action(zdt_icons::MAXIMIZE, "Fit", move || camera.fit()),
                Face::divider(),
                Face::toggle(
                    zdt_icons::PENCIL,
                    "Edit",
                    move || state.editing.get(),
                    move || {
                        let on = !state.editing.get_untracked();
                        state.editing.set(on);
                        if !on {
                            state.selected.set(None);
                            state.painting.set(false);
                        }
                    },
                ),
            ];
            if state.editing.get() {
                let workspace = workspace.clone();
                tools.extend([
                    Face::toggle(
                        zdt_icons::MOUSE_POINTER,
                        "Select tool",
                        move || state.tool.get() == Tool::Select,
                        move || state.tool.set(Tool::Select),
                    ),
                    Face::toggle(
                        zdt_icons::SPLINE,
                        "Node tool",
                        move || state.tool.get() == Tool::Nodes,
                        move || state.tool.set(Tool::Nodes),
                    ),
                    Face::toggle(
                        zdt_icons::PAINTBRUSH,
                        "Fill and stroke",
                        move || state.painting.get(),
                        move || state.painting.set(!state.painting.get_untracked()),
                    ),
                    Face::divider(),
                    Face::action(zdt_icons::TRASH, "Delete the selection", {
                        let workspace = workspace.clone();
                        move || {
                            let Some(at) = state.selected.get_untracked() else {
                                return;
                            };
                            let edit = state.model.with_untracked(|model| {
                                model.as_ref().and_then(|held| held.remove(at))
                            });
                            if let Some(edit) = edit {
                                state.selected.set(None);
                                commit(&workspace, buffer, state, edit);
                            }
                        }
                    }),
                ]);
            }
            view! { Toolbar(tools = tools, label = "SVG tools") }.any()
        }
    };

    // What the corner has to say: parse failures, and parts the renderer drops.
    let notice = move || state.notice.get();

    // The keys. Everything the preview answers goes through the region's keymap, with the base
    // map layered underneath, so the pill's toggle and the window keys keep working.
    let vim = crate::vim::use_vim();
    let on_key = move |event: &mut EventCx<'_, events::KeyDown>| {
        if let Some(chord) = crate::keys::chord_of(event, event.modifiers)
            && vim.key_in_region(chord, REGION)
        {
            event.prevent_default();
        }
        event.stop_propagation();
    };

    view! {
        column(
            class = "svgview",
            node_ref = node,
            tabindex = Focus::Programmatic,
            a11y:role = Role::Document,
            a11y:label = "SVG preview",
            on:key_down = on_key
        ) {
            {move || toolbar()}
            {move || {
                if state.editing.get() {
                    view! { SvgEditor(window = window, buffer = buffer, state = state, camera = camera) }
                        .any()
                } else {
                    ().any()
                }
            }}
            {move || {
                notice().map(|words| view! {
                    label(class = "svgview__notice") { {words} }
                }.any()).unwrap_or_else(|| ().any())
            }}
            Stage(camera = camera) {
                vector(
                    class = "svgview__drawing",
                    prop:svg = move || zgui::vocab::PropValue::from(state.render.get())
                )
            }
        }
    }
    .any()
}
