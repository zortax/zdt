//! The arrangement of windows on screen.
//!
//! One layout tree becomes one nesting of resizable panel groups. Sizes are percentages both here
//! and there, so a dragged handle reports exactly what the tree holds and the two never have to be
//! converted between.

use zgui::prelude::*;
use zgui::reactive::UnsyncCallback;
use zgui::{component, view};
use zgui_ui::prelude::*;
use zgui_ui_primitives::Orientation;

use crate::workspace::pane::PaneProps;
use crate::workspace::{Axis, Layout, Shape, WindowId, use_workspace};
use zdt_view::Erase;

/// Every window, arranged.
#[component]
pub fn Panes() -> impl IntoView {
    let workspace = use_workspace();
    // The arrangement, held apart from the shares so that this rebuilds on the first and not on
    // the second. A dragged divider reports new shares on every pointer move, and rebuilding takes
    // the handle out from under the drag. The shares are read below without subscribing: from the
    // first frame they belong to the panel group.
    let shape: zgui::reactive::RwSignal<Shape, zgui::reactive::LocalStorage> =
        zgui::reactive::RwSignal::new_local(workspace.layout_untracked().shape());
    let watching = {
        let workspace = workspace.clone();
        zgui::reactive::RenderEffect::new(move |_| {
            let next = workspace.shape();
            if shape.with_untracked(|held| *held != next) {
                shape.set(next);
            }
        })
    };
    on_cleanup_local(move || drop(watching));
    view! {
        box(class = "panes") {
            {move || {
                shape.get();
                tree(&workspace.layout_untracked())
            }}
        }
    }
}

/// One node of the layout, as a view.
///
/// A function, and no component, because it recurses. A component would need its own props type
/// at every level, and would remount the whole subtree whenever the branch above it changed.
fn tree(layout: &Layout) -> AnyView {
    match layout {
        Layout::Leaf(window) => view! { Pane(window = *window) }.any(),
        Layout::Split { axis, children } => {
            let direction = match axis {
                Axis::Horizontal => Orientation::Horizontal,
                Axis::Vertical => Orientation::Vertical,
            };
            // The first window in the group names it, which is what tells the workspace which
            // split a set of sizes belongs to.
            let first = first_window(&children[0].0);
            let workspace = use_workspace();
            let on_change = UnsyncCallback::new(move |sizes: Vec<f64>| {
                if let Some(first) = first {
                    workspace.resize(first, &sizes);
                }
            });

            let panels: Vec<AnyView> = children
                .iter()
                .enumerate()
                .flat_map(|(index, (child, size))| {
                    let mut views = Vec::new();
                    if index > 0 {
                        // The keyboard goes back to the editor when the drag ends: the handle is a
                        // tab stop of the library's, and a window left with the keyboard on a
                        // divider is one where the next motion goes unheard.
                        let workspace = use_workspace();
                        views.push(
                            view! {
                                ResizableHandle(
                                    on:pointer_up = move |_: &mut EventCx<'_, events::PointerUp>| {
                                        workspace.focus().reproject();
                                    }
                                )
                            }
                            .any(),
                        );
                    }
                    let inner = tree(child);
                    views.push(
                        view! {
                            ResizablePanel(default_size = *size, min_size = 8.0) { {inner} }
                        }
                        .any(),
                    );
                    views
                })
                .collect();

            view! {
                ResizablePanelGroup(direction = direction, on_change = on_change) {
                    {panels}
                }
            }
            .any()
        }
    }
}

/// The first window under a node, which is the one a split is named by.
fn first_window(layout: &Layout) -> Option<WindowId> {
    layout.windows().first().copied()
}
