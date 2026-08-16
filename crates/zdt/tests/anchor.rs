//! Placing a surface against the caret, and keeping it placed.
//!
//! What is asserted: a surface is solved rather than roughly guessed, and stays placed across its
//! own surface being rebound and taken away.
//!
//! These do *not* reproduce the disposal that motivated them — the testkit's frame does not tear
//! scopes down the way a real window's does, so they pass against the unfixed `place` too. The
//! rule they stand for is worth keeping in mind anyway: **never start an observation, or anything
//! else you mean to keep, inside a render effect.**

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use zdt::ui::anchor::{Anchoring, Placed, place};
use zgui::prelude::*;
use zgui::view;
use zgui_testkit_view::Window;

/// Somewhere in the text for a surface to sit under.
fn caret() -> zgui_editor::CaretRect {
    zgui_editor::CaretRect {
        x: 100.0,
        y: 200.0,
        width: 2.0,
        height: 16.0,
    }
}

/// A window, a placement, and the handle the placed surface binds.
struct Fixture {
    window: Window,
    placed: Placed,
    surface: NodeRef,
    /// The surfaces mounted so far, kept only so that they can be taken away again.
    mounted: RefCell<Vec<Box<dyn Any>>>,
}

impl Fixture {
    fn open() -> Self {
        let window = Window::open();
        window.place(window.root, 0.0, 0.0, 800.0, 600.0);

        let taken: Rc<RefCell<Option<(Placed, NodeRef)>>> = Rc::new(RefCell::new(None));
        {
            let taken = Rc::clone(&taken);
            window.scope.with(|| {
                let surface = NodeRef::new();
                let placed = place(surface, || Some(caret()), Anchoring::default());
                *taken.borrow_mut() = Some((placed, surface));
            });
        }
        let (placed, surface) = taken.borrow_mut().take().expect("the placement was made");
        Self {
            window,
            placed,
            surface,
            mounted: RefCell::new(Vec::new()),
        }
    }

    /// Mounts one surface bound to the shared handle, and gives it a size to be placed by.
    fn mount_a_surface(&self) {
        let surface = self.surface;
        let built = self.window.scope.with(|| {
            let view = view! { box(node_ref = surface) };
            let mut built = view.into_view().build(&mut self.window.cx.cx());
            built.mount(&self.window.dom_handle, self.window.root, None);
            built
        });
        self.mounted.borrow_mut().push(Box::new(built));
        self.window.frame();

        if let Some(node) = surface.get_untracked() {
            self.window.place(node, 0.0, 0.0, 320.0, 180.0);
        }
        self.window.frame();
    }

    /// Takes the oldest surface away, which is what a panel finishing its exit does.
    fn drop_the_oldest_surface(&self) {
        let taken = self.mounted.borrow_mut().remove(0);
        drop(taken);
        self.window.frame();
    }

    /// Where it says the surface goes, read the way a view attribute reads it.
    fn top(&self) -> Option<f32> {
        self.top_of(self.placed)
    }

    /// The same, for a placement made later.
    fn top_of(&self, placed: Placed) -> Option<f32> {
        self.window.scope.with(move || placed.top.get())
    }

    /// Whether a placement was solved, rather than roughly guessed by the fallback.
    fn solved(&self, placed: Placed) -> bool {
        self.window.scope.with(move || placed.settled.get())
    }

    /// A second placement against the same handle, made *now* — with the surface already bound.
    fn place_again(&self) -> Placed {
        let surface = self.surface;
        let taken: Rc<RefCell<Option<Placed>>> = Rc::new(RefCell::new(None));
        {
            let taken = Rc::clone(&taken);
            self.window.scope.with(move || {
                *taken.borrow_mut() = Some(place(surface, || Some(caret()), Anchoring::default()));
            });
        }
        self.window.frame();
        taken.borrow_mut().take().expect("a second placement")
    }
}

#[test]
fn a_measured_surface_is_placed_against_the_caret() {
    let fixture = Fixture::open();
    fixture.mount_a_surface();

    let top = fixture.top().expect("somewhere to be");
    // Under the caret's line, which is where a surface with room below it goes.
    assert!(top >= caret().y, "{top} is not below the caret");
}

#[test]
fn it_is_still_placed_after_its_surface_goes_away() {
    // A panel that is leaving keeps its view but not its element, so the shared handle unbinds and
    // the effect inside `place` re-runs while the placement is still being read.
    let fixture = Fixture::open();
    fixture.mount_a_surface();
    assert!(fixture.top().is_some(), "placed once");

    fixture.mount_a_surface();
    fixture.drop_the_oldest_surface();

    assert!(
        fixture.top().is_some(),
        "the placement must survive its surface being taken away"
    );
}

#[test]
fn it_survives_being_opened_and_closed_many_times() {
    // Opened and dismissed all day, which is what a documentation popover is for.
    let fixture = Fixture::open();
    for _ in 0..6 {
        fixture.mount_a_surface();
        assert!(fixture.top().is_some());
        fixture.drop_the_oldest_surface();
        assert!(fixture.top().is_some());
    }
}

#[test]
fn a_placement_made_against_a_bound_surface_keeps_its_viewport() {
    // The surface is already bound when the placement is made, so the window's rectangle is
    // acquired on the effect's first run — and then something re-runs that effect.
    let fixture = Fixture::open();
    fixture.mount_a_surface();

    let second = fixture.place_again();
    assert!(fixture.solved(second), "solved on the first measurement");

    // Anything at all that re-runs the effect: here, the handle being bound again.
    fixture.mount_a_surface();

    assert!(
        fixture.solved(second),
        "the window rectangle must outlive a re-run of the effect that fetched it"
    );
    assert!(fixture.top_of(second).is_some());
}

#[test]
fn a_placement_is_solved_rather_than_guessed() {
    // The fallback in `place` is deliberately invisible — a surface it could not solve is still
    // put somewhere — so a placement that quietly stopped solving would look exactly like one that
    // worked. This is what says the solver is the thing being exercised above.
    let fixture = Fixture::open();
    fixture.mount_a_surface();

    let placed = fixture.placed;
    assert!(
        fixture.window.scope.with(move || placed.settled.get()),
        "the solver answered"
    );
    assert_eq!(
        fixture
            .window
            .scope
            .with(move || placed.side.get())
            .as_deref(),
        Some("bottom"),
        "under the caret, where there is room"
    );
}

/// A surface that places itself, built the way the application builds one: inside a component, so
/// the placement belongs to a component's scope rather than to the window's.
#[zgui::component]
fn Anchored(
    /// The handle every surface here shares, as the presence shares one across two panels.
    surface: NodeRef,
    /// Where to hand the placement back to the test.
    taken: Rc<RefCell<Option<Placed>>>,
) -> impl IntoView {
    let placed = place(surface, || Some(caret()), Anchoring::default());
    *taken.borrow_mut() = Some(placed);

    view! {
        box(
            node_ref = surface,
            // The attributes matter: reading the placement from one of these, mid-teardown, is
            // where the application died.
            attr:data-side = move || placed.side.get(),
            style:left = placed.left_px(),
            style:top = placed.top_px(),
        )
    }
}

/// Builds one [`Anchored`], mounts it, and gives its element a size to be placed against.
fn anchor_one(window: &Window, surface: NodeRef) -> (Box<dyn Any>, Placed) {
    let taken: Rc<RefCell<Option<Placed>>> = Rc::new(RefCell::new(None));
    let built = {
        let taken = Rc::clone(&taken);
        window.scope.with(|| {
            let view = view! { Anchored(surface = surface, taken = taken.clone()) };
            let mut built = view.into_view().build(&mut window.cx.cx());
            built.mount(&window.dom_handle, window.root, None);
            built
        })
    };
    window.frame();
    if let Some(node) = surface.get_untracked() {
        window.place(node, 0.0, 0.0, 320.0, 180.0);
    }
    window.frame();

    let placed = taken.borrow_mut().take().expect("the placement was made");
    (Box::new(built), placed)
}

#[test]
fn a_second_anchored_surface_keeps_its_viewport() {
    // Two panels sharing one handle: the second is built while the first is still up, so it finds
    // the handle already bound and is re-run when its own element binds it.
    let window = Window::open();
    window.place(window.root, 0.0, 0.0, 800.0, 600.0);

    let surface = window.scope.with(NodeRef::new);
    let (first, _) = anchor_one(&window, surface);
    let (second, placed) = anchor_one(&window, surface);

    // The first one goes away, as a panel that has finished leaving does.
    drop(first);
    window.frame();

    let solved = window.scope.with(move || placed.settled.get());
    let side = window.scope.with(move || placed.side.get());
    assert!(solved, "the second surface still knows where the window is");
    assert_eq!(side.as_deref(), Some("bottom"));

    drop(second);
}
