//! The image preview: the file on disk, drawn on a stage, with the edit tools over it.
//!
//! An image buffer has only this form. The preview decodes nothing itself: the image element
//! probes and decodes off the interface thread at the size the stage gives it, so zooming in is
//! a fresh decode at a fresh size class and stays sharp.

mod crop;
mod edit;

pub use edit::Edits;

use std::cell::RefCell;
use std::rc::Rc;

use rustc_hash::FxHashMap;
use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RenderEffect, RwSignal};
use zgui::task::blocking;
use zgui::{component, view};

use crate::rich::stage::{Camera, StageProps, use_stages};
use crate::rich::toolbar::{Tool, ToolbarProps};
use crate::workspace::{BufferId, BufferKind, WindowId, Workspace, use_workspace};
use crop::CropOverlayProps;

/// The keymap overlay a focused image preview answers keys in.
pub const REGION: &str = "image";

/// One mounted image preview's state.
#[derive(Clone, Copy, PartialEq)]
struct ImageView {
    /// The camera on its stage.
    camera: Camera,
    /// Its pending edits.
    edits: Edits,
    /// The picture's pixel size, once the header has been read.
    natural: RwSignal<Option<(u32, u32)>, LocalStorage>,
}

/// Every mounted image preview, by the window and buffer it belongs to.
///
/// No signal, the same reasoning as [`super::Previews`]: nothing on screen is decided by which
/// previews exist, and a key that edits one needs it right now.
#[derive(Clone)]
pub struct ImageViews {
    inner: Rc<RefCell<FxHashMap<(WindowId, BufferId), ImageView>>>,
}

impl ImageViews {
    /// Nothing mounted.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Rc::new(RefCell::new(FxHashMap::default())),
        }
    }

    fn register(&self, window: WindowId, buffer: BufferId, view: ImageView) {
        self.inner.borrow_mut().insert((window, buffer), view);
    }

    fn forget(&self, window: WindowId, buffer: BufferId, view: ImageView) {
        let mut held = self.inner.borrow_mut();
        if held.get(&(window, buffer)) == Some(&view) {
            held.remove(&(window, buffer));
        }
    }

    /// The preview the keyboard is in, when it is in one.
    fn current(&self, workspace: &Workspace) -> Option<(BufferId, ImageView)> {
        let window = workspace.focused_untracked();
        let buffer = workspace.buffer_in_untracked(window)?;
        let view = self.inner.borrow().get(&(window, buffer)).copied()?;
        Some((buffer, view))
    }

    /// The preview of `buffer` under the keyboard, or any other split showing it.
    fn of_buffer(&self, workspace: &Workspace, buffer: BufferId) -> Option<ImageView> {
        let focused = workspace.focused_untracked();
        let held = self.inner.borrow();
        held.get(&(focused, buffer)).copied().or_else(|| {
            held.iter()
                .find(|((_, shown), _)| *shown == buffer)
                .map(|(_, view)| *view)
        })
    }

    /// Runs `act` on every mounted preview of `buffer`.
    fn each_of(&self, buffer: BufferId, mut act: impl FnMut(&ImageView)) {
        for ((_, shown), view) in self.inner.borrow().iter() {
            if *shown == buffer {
                act(view);
            }
        }
    }

    /// Whether any split still has edits pending on `buffer`.
    fn any_pending(&self, buffer: BufferId) -> bool {
        self.inner
            .borrow()
            .iter()
            .any(|((_, shown), view)| *shown == buffer && view.edits.snapshot().is_some())
    }

    /// Writes what the buffer's dirty mark should say, from every split's pending edits.
    fn refresh_dirty(&self, workspace: &Workspace, buffer: BufferId) {
        let Some(entry) = workspace.buffer_untracked(buffer) else {
            return;
        };
        let pending = self.any_pending(buffer);
        if entry.dirty.get_untracked() != pending {
            entry.dirty.set(pending);
        }
    }
}

impl Default for ImageViews {
    fn default() -> Self {
        Self::new()
    }
}

/// Puts the registry where every component can find it.
pub fn provide(views: ImageViews) {
    zgui::reactive::provide_local_context(views);
}

/// It, from inside a component.
fn use_views() -> Option<ImageViews> {
    zgui::reactive::use_local_context::<ImageViews>()
}

/// Editing the image preview under the keyboard, from the keys of its region.
pub fn run(workspace: &Workspace, leaf: &str) {
    let Some(views) = use_views() else {
        return;
    };
    let Some((buffer, view)) = views.current(workspace) else {
        return;
    };
    let edits = view.edits;

    match leaf {
        "edit" => edits.set_editing(!edits.editing()),
        "crop" if edits.editing() => edits.set_cropping(!edits.cropping()),
        "rotate_cw" if edits.editing() => edits.rotate(1),
        "rotate_ccw" if edits.editing() => edits.rotate(-1),
        "flip_h" if edits.editing() => edits.flip_horizontal(),
        "flip_v" if edits.editing() => edits.flip_vertical(),
        "apply" if edits.editing() => apply(workspace, buffer, view),
        "discard" if edits.editing() => {
            edits.reset();
            views.refresh_dirty(workspace, buffer);
        }
        "escape" => {
            if edits.cropping() {
                edits.set_cropping(false);
            } else if edits.editing() {
                edits.set_editing(false);
            }
        }
        // Silently. The base map layers underneath the region, and an unbound key there falls
        // through to it.
        _ => {}
    }
}

/// Writes `buffer` back to its file, which `:w` comes to for an image.
pub fn save(workspace: &Workspace, buffer: BufferId) {
    let Some(views) = use_views() else {
        return;
    };
    match views.of_buffer(workspace, buffer) {
        Some(view) if view.edits.snapshot().is_some() => apply(workspace, buffer, view),
        _ => {
            if let Some(path) = workspace
                .buffer_untracked(buffer)
                .and_then(|held| held.path)
            {
                workspace.say(format!("{} unchanged", path.display()));
            }
        }
    }
}

/// Applies `view`'s pending edits to the file, re-encoding it on a worker.
fn apply(workspace: &Workspace, buffer: BufferId, view: ImageView) {
    let Some(entry) = workspace.buffer_untracked(buffer) else {
        return;
    };
    let Some(path) = entry.path.clone() else {
        return;
    };
    let BufferKind::Image { shown } = entry.kind else {
        return;
    };
    let Some(snapshot) = view.edits.snapshot() else {
        workspace.say("nothing to apply");
        return;
    };
    let Some(views) = use_views() else {
        return;
    };

    let workspace = workspace.clone();
    // Detached: the toggle that asked may be unmounted before the write lands, and a write
    // cancelled half way is a file lost.
    zdt_view::detached(async move {
        let writing = path.clone();
        let written = blocking(move || edit::write(&writing, &snapshot)).await;

        match written {
            Ok((bytes, (width, height))) => {
                // A fresh registration is a fresh `src`: every element showing this buffer drops
                // the old picture for the written one.
                shown.set(Some(zgui_image::ImageBytes::new(bytes)));
                view.edits.reset();
                views.each_of(buffer, |held| held.natural.set(Some((width, height))));
                views.refresh_dirty(&workspace, buffer);
                if let Some(git) = zgui::reactive::use_local_context::<crate::git::Git>() {
                    git.refresh_soon(buffer);
                }
                if let Some(status) = zgui::reactive::use_local_context::<crate::git::Status>() {
                    status.refresh_soon();
                }
                workspace.say(format!("{} written", path.display()));
            }
            Err(error) => workspace.complain(format!("{}: {}", path.display(), error)),
        }
    });
}

#[component]
pub fn ImagePreview(
    /// Which window it is in.
    window: WindowId,
    /// Which buffer it draws.
    buffer: BufferId,
) -> impl IntoView {
    use zdt_view::Erase;

    let workspace = use_workspace();
    let Some(entry) = workspace.buffer_untracked(buffer) else {
        // The buffer closed between the window listing it and this mounting. Nothing to show.
        return view! { box() }.any();
    };
    let Some(path) = entry.path.clone() else {
        return view! { box() }.any();
    };
    let BufferKind::Image { shown } = entry.kind else {
        return view! { box() }.any();
    };
    let name = entry.name();
    // A gif is animated, and applying edits would write back the first frame alone.
    let editable = path
        .extension()
        .and_then(|held| held.to_str())
        .is_none_or(|held| !held.eq_ignore_ascii_case("gif"));

    // Where the keyboard lands. The projector prefers this sink over an editor handle, and an
    // image buffer never has one.
    let node = NodeRef::new();
    crate::focus::claim::sink(
        crate::focus::Spot::Buffer(window, buffer),
        crate::focus::Sink::Node(node),
    );

    let camera = Camera::new();
    let edits = Edits::new();
    let natural: RwSignal<Option<(u32, u32)>, LocalStorage> = RwSignal::new_local(None);
    let mine = ImageView {
        camera,
        edits,
        natural,
    };

    let stages = use_stages();
    let views = use_views();
    stages.register(window, buffer, camera);
    if let Some(views) = &views {
        views.register(window, buffer, mine);
    }
    on_cleanup_local({
        let stages = stages.clone();
        let views = views.clone();
        move || {
            stages.forget(window, buffer, camera);
            if let Some(views) = &views {
                views.forget(window, buffer, mine);
            }
        }
    });

    // The pixel size, read from the header alone. The pixels wait for the stage.
    {
        let probing = path.clone();
        zdt_view::detached(async move {
            if let Ok(size) = blocking(move || zgui_image::probe_file(&probing)).await {
                natural.set(Some((size.width, size.height)));
            }
        });
    }

    // The stage fits the *drawn* footprint, which a pending quarter turn swaps.
    {
        let sizing = RenderEffect::new(move |_| {
            let Some((width, height)) = natural.get() else {
                return;
            };
            let (width, height) = if edits.quarter() % 2 == 1 {
                (height, width)
            } else {
                (width, height)
            };
            camera.set_content(width as f32, height as f32);
        });
        on_cleanup_local(move || drop(sizing));
    }

    // The buffer's dirty mark follows the pending edits, over every split showing it.
    {
        let workspace = workspace.clone();
        let marking = RenderEffect::new(move |_| {
            let _ = edits.is_pending();
            if let Some(views) = use_views() {
                views.refresh_dirty(&workspace, buffer);
            }
        });
        on_cleanup_local(move || drop(marking));
    }

    // What the element decodes: the file, until a save replaces it with the written bytes.
    let src = {
        let path = path.to_string_lossy().into_owned();
        move || {
            Some(match shown.get() {
                Some(bytes) => bytes.url(),
                None => path.clone(),
            })
        }
    };

    // The picture inside the plane. The plane wears the drawn footprint; a turned picture is
    // laid out at its own footprint and turned into place.
    let footprint = move |of: fn((f32, f32)) -> f32| {
        move || {
            let (_, _, width, height) = camera.placement()?;
            let swapped = edits.quarter() % 2 == 1;
            let (wide, tall) = if swapped {
                (height, width)
            } else {
                (width, height)
            };
            Some(format!("{}px", of((wide, tall))))
        }
    };
    let offset = move |of: fn((f32, f32, f32, f32)) -> f32| {
        move || {
            let (_, _, width, height) = camera.placement()?;
            let swapped = edits.quarter() % 2 == 1;
            let (wide, tall) = if swapped {
                (height, width)
            } else {
                (width, height)
            };
            Some(format!("{}px", of((width, height, wide, tall)) / 2.0))
        }
    };
    let turning = move || {
        let (flip_h, flip_v) = edits.flips();
        let quarter = edits.quarter();
        let mut turns = String::new();
        if flip_h {
            turns.push_str("scaleX(-1) ");
        }
        if flip_v {
            turns.push_str("scaleY(-1) ");
        }
        if quarter != 0 {
            turns.push_str(&format!("rotate({}deg)", u16::from(quarter) * 90));
        }
        (!turns.is_empty()).then(|| turns.trim_end().to_owned())
    };

    // The toolbar: the camera tools, and the edit tools while they are out.
    let toolbar = {
        let workspace = workspace.clone();
        move || {
            let mut tools = vec![
                Tool::action(zdt_icons::ZOOM_OUT, "Zoom out", move || {
                    camera.zoom_by(1.0 / crate::rich::stage::STEP, None);
                }),
                Tool::action(zdt_icons::ZOOM_IN, "Zoom in", move || {
                    camera.zoom_by(crate::rich::stage::STEP, None);
                }),
                Tool::action(zdt_icons::MAXIMIZE, "Fit", move || camera.fit()),
                Tool::action(zdt_icons::SQUARE, "Actual size", move || camera.actual()),
            ];
            if editable {
                tools.push(Tool::divider());
                tools.push(Tool::toggle(
                    zdt_icons::PENCIL,
                    "Edit",
                    move || edits.editing(),
                    move || edits.set_editing(!edits.editing()),
                ));
            }
            if editable && edits.editing() {
                let workspace = workspace.clone();
                tools.extend([
                    Tool::toggle(
                        zdt_icons::CROP,
                        "Crop",
                        move || edits.cropping(),
                        move || edits.set_cropping(!edits.cropping()),
                    ),
                    Tool::action(
                        zdt_icons::ROTATE_CCW,
                        "Rotate counter-clockwise",
                        move || {
                            edits.rotate(-1);
                        },
                    ),
                    Tool::action(zdt_icons::ROTATE_CW, "Rotate clockwise", move || {
                        edits.rotate(1);
                    }),
                    Tool::action(zdt_icons::FLIP_HORIZONTAL, "Flip horizontal", move || {
                        edits.flip_horizontal();
                    }),
                    Tool::action(zdt_icons::FLIP_VERTICAL, "Flip vertical", move || {
                        edits.flip_vertical();
                    }),
                    Tool::divider(),
                    Tool::action(zdt_icons::CHECK, "Apply and write", {
                        let workspace = workspace.clone();
                        move || {
                            if let Some(views) = use_views()
                                && let Some((buffer, view)) = views.current(&workspace)
                            {
                                apply(&workspace, buffer, view);
                            }
                        }
                    }),
                    Tool::action(zdt_icons::X, "Discard edits", {
                        let workspace = workspace.clone();
                        move || {
                            edits.reset();
                            if let Some(views) = use_views() {
                                views.refresh_dirty(&workspace, buffer);
                            }
                        }
                    }),
                ]);
            }
            view! { Toolbar(tools = tools, label = "Image tools") }.any()
        }
    };

    // What is being looked at, in the corner: the pixel size and the scale.
    let readout = move || {
        let (width, height) = natural.get().unwrap_or((0, 0));
        let percent = (camera.scale() * 100.0).round();
        if width == 0 {
            String::new()
        } else {
            format!("{width} × {height} · {percent}%")
        }
    };

    // The keys. Everything the preview answers goes through the region's keymap, with the base
    // map layered underneath, so the window keys keep working from in here.
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
            class = "imgview",
            node_ref = node,
            tabindex = Focus::Programmatic,
            a11y:role = Role::Image,
            a11y:label = name,
            on:key_down = on_key
        ) {
            {move || toolbar()}
            Stage(camera = camera) {
                // The braced child first: a braced child after a parenthesised element would
                // read as that element's children block.
                {move || {
                    if edits.cropping() {
                        view! { CropOverlay(camera = camera, edits = edits) }.any()
                    } else {
                        ().any()
                    }
                }}
                image(
                    class = "imgview__picture",
                    src = src,
                    style:width = footprint(|held| held.0),
                    style:height = footprint(|held| held.1),
                    style:left = offset(|held| held.0 - held.2),
                    style:top = offset(|held| held.1 - held.3),
                    style:transform = turning,
                    a11y:hidden = true
                )
            }
            label(class = "imgview__readout") { {readout} }
        }
    }
    .any()
}
