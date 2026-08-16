//! The settings page, as a page: mounted, pressed, and asked whether anything changed.
//!
//! Two things are guarded. First, that a control is wired to the setting it names, which is
//! ordinary. Second, that no floating panel is centred by a transform, which is not. A transformed
//! box's descendants are tested against a clip measured before the box moved, so everything inside
//! one past a boundary partway across it paints correctly and cannot be pressed.

use std::cell::RefCell;
use std::rc::Rc;

use zdt::settings::Settings;
use zdt::settings::view::ConfigPanelProps;
use zgui::prelude::*;
use zgui::view;
use zgui_testkit_view::Window;

/// A window with the settings page in it, and the settings behind it.
fn panel() -> (Window, Settings) {
    let window = Window::open();
    window.place(window.root, 0.0, 0.0, 1000.0, 700.0);

    let taken: Rc<RefCell<Option<Settings>>> = Rc::new(RefCell::new(None));
    let built = {
        let taken = Rc::clone(&taken);
        window.scope.with(|| {
            let settings = Settings::new(zdt_core::Config::default(), None);
            zdt::settings::provide(settings.clone());
            *taken.borrow_mut() = Some(settings);

            let view = view! { ConfigPanel() };
            let mut built = view.into_view().build(&mut window.cx.cx());
            built.mount(&window.dom_handle, window.root, None);
            built
        })
    };
    std::mem::forget(built);
    window.frame();

    let settings = taken.borrow_mut().take().expect("the settings were made");
    (window, settings)
}

/// Every node under `from`, depth first.
fn every_node(window: &Window, from: zgui::view::NodeId) -> Vec<zgui::view::NodeId> {
    let mut found = vec![from];
    for child in window.dom.tree().children(from) {
        found.extend(every_node(window, child));
    }
    found
}

/// Every node whose semantics say it plays `role`.
fn every(window: &Window, role: &str) -> Vec<zgui::view::NodeId> {
    every_node(window, window.root)
        .into_iter()
        .filter(|node| {
            window
                .dom
                .tree()
                .semantics(*node)
                .is_some_and(|found| format!("{:?}", found.role) == role)
        })
        .collect()
}

/// Presses `node` the way a pointer does.
fn press(window: &Window, node: zgui::view::NodeId) {
    let at = zgui::geom::Point::new(zgui::geom::CssPx(0.0), zgui::geom::CssPx(0.0));
    let dispatcher = window.dispatcher();
    for kind in [
        zgui::vocab::EventKind::PointerDown,
        zgui::vocab::EventKind::PointerUp,
        zgui::vocab::EventKind::Click,
    ] {
        dispatcher.send_to(
            node,
            kind,
            zgui::vocab::Payload::Pointer(zgui::vocab::PointerEvent::mouse(at)),
        );
    }
    window.frame();
}

#[test]
fn the_page_puts_its_controls_on_the_page() {
    // Nothing subtle: the panes are bindings and a pane that built none of them would pass every
    // other test here by having nothing to press.
    let (window, _settings) = panel();
    assert!(!every(&window, "Switch").is_empty(), "switches");
    assert!(!every(&window, "ComboBox").is_empty(), "choosers");
    assert!(!every(&window, "Slider").is_empty(), "sliders");
    assert!(!every(&window, "Tab").is_empty(), "the page list");
}

#[test]
fn a_switch_changes_the_setting_it_is_bound_to() {
    // The panel is bindings over the settings and nothing else, so a switch that does not move the
    // settings is a switch that does nothing at all.
    let (window, settings) = panel();
    let switches = every(&window, "Switch");

    let before = settings.with_untracked(|config| config.ui.notifications);
    // The second switch on the appearance page is the one for announcements.
    press(&window, switches[1]);
    let after = settings.with_untracked(|config| config.ui.notifications);

    assert_ne!(
        before, after,
        "pressing a switch changes what it is bound to"
    );
}

/// Every surface that floats over the window and holds something worth pressing.
const FLOATING: &[&str] = &[
    ".config__modal",
    ".picker",
    ".git--modal",
    ".prompt",
    ".termfloat",
];

#[test]
fn no_floating_panel_rests_on_a_transform() {
    // Only these surfaces, and only their resting state: a transform on a leaf with nothing inside
    // it is harmless, and one inside `@keyframes` lasts ninety milliseconds.
    let sheet = zdt::assets::sheet();

    let mut depth = 0_i32;
    let mut in_keyframes = false;
    let mut selector = String::new();
    let mut offenders = Vec::new();

    for line in sheet.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("@keyframes") {
            in_keyframes = true;
        }
        if !in_keyframes && depth == 0 && trimmed.ends_with('{') {
            selector = trimmed.trim_end_matches('{').trim().to_owned();
        }
        let names_a_panel = FLOATING.iter().any(|panel| {
            selector
                .split([',', ' ', ':', '['])
                .any(|part| part == *panel)
        });
        if !in_keyframes
            && depth > 0
            && names_a_panel
            && trimmed.starts_with("transform:")
            && !trimmed.starts_with("transform-origin")
        {
            offenders.push(format!("{selector} — {trimmed}"));
        }
        depth += i32::try_from(line.matches('{').count()).unwrap_or(0);
        depth -= i32::try_from(line.matches('}').count()).unwrap_or(0);
        if in_keyframes && depth == 0 {
            in_keyframes = false;
        }
    }

    assert!(
        offenders.is_empty(),
        "a panel centred by a transform cannot be pressed past a boundary partway across it; \
         centre with `left: 0; right: 0; margin: 0 auto` instead:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn the_guard_would_notice_a_transform_coming_back() {
    // The search above is over a style sheet, and the kind that quietly stops matching. This runs
    // it against a rule that should fail.
    let was = ".config__modal {\n    position: absolute;\n    left: 50%;\n    \
               transform: translateX(-50%);\n}\n";

    let mut depth = 0_i32;
    let mut selector = String::new();
    let mut offenders = Vec::new();
    for line in was.lines() {
        let trimmed = line.trim();
        if depth == 0 && trimmed.ends_with('{') {
            selector = trimmed.trim_end_matches('{').trim().to_owned();
        }
        let names_a_panel = FLOATING.iter().any(|panel| {
            selector
                .split([',', ' ', ':', '['])
                .any(|part| part == *panel)
        });
        if depth > 0 && names_a_panel && trimmed.starts_with("transform:") {
            offenders.push(trimmed.to_owned());
        }
        depth += i32::try_from(line.matches('{').count()).unwrap_or(0);
        depth -= i32::try_from(line.matches('}').count()).unwrap_or(0);
    }

    assert_eq!(offenders, vec!["transform: translateX(-50%);".to_owned()]);
}
