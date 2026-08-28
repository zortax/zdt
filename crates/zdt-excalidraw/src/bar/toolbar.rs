//! One floating capsule of icon controls.
//!
//! The same shape as the editor's other rich views: a row of outlines at the top, each lit while
//! the thing it stands for is on. A press bubbles on to whatever holds the editor, which focuses
//! the split it is in.

use std::rc::Rc;

use zdt_icons::IconProps;
use zgui::prelude::*;
use zgui::{component, view};

/// One control on a toolbar.
#[derive(Clone)]
pub struct Tool {
    /// The outline it is drawn with.
    pub icon: &'static str,
    /// What it is called, for assistive technology and for the eye.
    pub label: &'static str,
    /// Whether it is lit, for a control that is a mode. A plain action has none.
    ///
    /// A closure over signals, so the face follows them.
    pub on: Option<Rc<dyn Fn() -> bool>>,
    /// What pressing it does.
    pub run: Rc<dyn Fn()>,
}

impl Tool {
    /// A plain action.
    pub fn action(icon: &'static str, label: &'static str, run: impl Fn() + 'static) -> Self {
        Self {
            icon,
            label,
            on: None,
            run: Rc::new(run),
        }
    }

    /// A mode that is lit while `on` answers true.
    pub fn toggle(
        icon: &'static str,
        label: &'static str,
        on: impl Fn() -> bool + 'static,
        run: impl Fn() + 'static,
    ) -> Self {
        Self {
            icon,
            label,
            on: Some(Rc::new(on)),
            run: Rc::new(run),
        }
    }

    /// A thin divider between groups.
    pub fn divider() -> Self {
        Self {
            icon: "",
            label: "",
            on: None,
            run: Rc::new(|| {}),
        }
    }

    /// Whether this is the divider.
    fn is_divider(&self) -> bool {
        self.icon.is_empty()
    }
}

/// The row.
///
/// `tools` is read once: a toolbar whose set of controls changes is two toolbars in a reactive
/// hole. What changes on a control is whether it is lit, and that is a closure on the control.
#[component]
pub fn Toolbar(
    /// The controls, in order.
    tools: Vec<Tool>,
    /// What the row is called.
    label: &'static str,
    /// Which class the capsule takes.
    #[prop(default = "exdrawbar")]
    class: &'static str,
) -> impl IntoView {
    use zdt_view::Erase;

    let faces: Vec<AnyView> = tools
        .into_iter()
        .map(|tool| view! { ToolFace(tool = tool) }.any())
        .collect();

    view! {
        row(class = class, a11y:role = Role::Toolbar, a11y:label = label) {
            {faces}
        }
    }
}

/// One control on the row.
#[component]
fn ToolFace(
    /// What it shows and does.
    tool: Tool,
) -> impl IntoView {
    use zdt_view::Erase;

    if tool.is_divider() {
        return view! { box(class = "exdrawbar__divider") {} }.any();
    }

    let lit = {
        let on = tool.on.clone();
        move || on.as_ref().and_then(|on| on().then(|| "true".to_owned()))
    };
    let run = Rc::clone(&tool.run);

    view! {
        control(
            class = "exdrawbar__tool",
            tabindex = Focus::Programmatic,
            a11y:label = tool.label,
            attr:data-on = lit,
            on:pointer_down = move |ev: &mut EventCx<'_, events::PointerDown>| {
                run();
                ev.stop_propagation();
            }
        ) {
            Icon(icon = tool.icon, class = "icon--xs")
        }
    }
    .any()
}
