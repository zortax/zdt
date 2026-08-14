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

use crate::ui::Erase;
use crate::ui::pane::PaneProps;
use crate::workspace::{Axis, Layout, WindowId, use_workspace};

/// Every window, arranged.
#[component]
pub fn Panes() -> impl IntoView {
    let workspace = use_workspace();
    view! {
        box(class = "panes") {
            {move || tree(&workspace.layout())}
        }
    }
}

/// One node of the layout, as a view.
///
/// Written as a function rather than a component because it recurses: a component would need its
/// own props type at every level and would remount the whole subtree whenever the branch above it
/// changed.
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
                        views.push(view! { ResizableHandle() }.any());
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
