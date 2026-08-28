//! The editor, mounted and drawn on.
//!
//! What is checked here is the thing unit tests cannot: that a press, a drag and a release reach
//! the drawing through the real document, and that one gesture is one change and not several.

use std::cell::RefCell;
use std::rc::Rc;

use excalidraw::{Id, Kind, Scene};
use kurbo::Point;
use zdt_excalidraw::state::{Board, Tool};
use zdt_excalidraw::view::EditorProps;
use zgui::prelude::*;
use zgui::view;
use zgui_testkit_view::Window;

/// A window with the editor in it, and the board behind it.
fn editor(elements: &str) -> (Window, Board) {
    let window = Window::open();
    window.place(window.root, 0.0, 0.0, 800.0, 600.0);

    let taken: Rc<RefCell<Option<Board>>> = Rc::new(RefCell::new(None));
    let built = {
        let taken = Rc::clone(&taken);
        window.scope.with(|| {
            let text = format!(r#"{{"type":"excalidraw","version":2,"elements":{elements}}}"#);
            let drawing = excalidraw::file::parse(&text).expect("a drawing");
            let board = Board::new(Scene::new(drawing, 1, 1));
            // Looking at the origin, one to one, so a point in the view is a point in the scene.
            board.viewport.set_size(800.0, 600.0);
            board.viewport.scroll_to(Point::ZERO);
            *taken.borrow_mut() = Some(board);

            let held = view! { Editor(board = board) };
            let mut built = held.into_view().build(&mut window.cx.cx());
            built.mount(&window.dom_handle, window.root, None);
            built
        })
    };
    std::mem::forget(built);
    window.frame();

    let board = taken.borrow_mut().take().expect("the board was made");
    (window, board)
}

/// Drags from one point to another, the way a pointer does.
fn drag(board: &Board, from: Point, to: Point) {
    let held = zdt_excalidraw::pointer::Held::default();
    zdt_excalidraw::pointer::down(board, from, held);
    zdt_excalidraw::pointer::moved(
        board,
        Point::new((from.x + to.x) / 2.0, (from.y + to.y) / 2.0),
        held,
    );
    zdt_excalidraw::pointer::moved(board, to, held);
    zdt_excalidraw::pointer::up(board);
}

#[test]
fn the_editor_puts_its_tools_on_the_screen() {
    let (window, _board) = editor("[]");
    let roles: Vec<String> = every_node(&window, window.root)
        .into_iter()
        .filter_map(|node| {
            window
                .dom
                .tree()
                .semantics(node)
                .map(|found| format!("{:?}", found.role))
        })
        .collect();
    assert!(roles.iter().any(|role| role == "Toolbar"), "the tool row");
    assert!(roles.iter().any(|role| role == "Document"), "the drawing");
}

#[test]
fn dragging_with_a_shape_tool_draws_one_shape() {
    let (_window, board) = editor("[]");
    board.tool.set(Tool::Rectangle);
    drag(&board, Point::new(100.0, 100.0), Point::new(300.0, 220.0));

    let scene = board.read_untracked();
    assert_eq!(scene.elements().len(), 1, "one shape, not several");
    let held = &scene.elements()[0];
    assert_eq!(held.kind, Kind::Rectangle);
    assert!((held.width - 200.0).abs() < 1e-6);
    assert!((held.height - 120.0).abs() < 1e-6);
    assert_eq!(
        board.revision.get_untracked(),
        1,
        "one gesture is one change"
    );
    assert_eq!(
        board.tool.get_untracked(),
        Tool::Select,
        "and the pointer goes back to choosing, so the new shape can be moved at once"
    );
}

/// The pen is the one tool that stays: drawing is a run of strokes, and choosing the pen again
/// between them is not how anyone draws.
#[test]
fn the_pen_stays_chosen_after_a_stroke() {
    let (_window, board) = editor("[]");
    board.tool.set(Tool::Freedraw);
    drag(&board, Point::new(100.0, 100.0), Point::new(300.0, 220.0));

    assert_eq!(board.tool.get_untracked(), Tool::Freedraw);
    assert!(
        !board.read_untracked().has_selection(),
        "and the stroke is not chosen either"
    );
}

#[test]
fn a_press_that_goes_nowhere_draws_nothing() {
    let (_window, board) = editor("[]");
    board.tool.set(Tool::Rectangle);
    drag(&board, Point::new(100.0, 100.0), Point::new(100.0, 100.0));
    assert!(board.read_untracked().elements().is_empty());
    assert_eq!(board.revision.get_untracked(), 0);
}

#[test]
fn dragging_a_shape_moves_it_once() {
    let (_window, board) = editor(
        r##"[{"type":"rectangle","id":"a","x":100,"y":100,"width":100,"height":100,
              "backgroundColor":"#a5d8ff"}]"##,
    );
    drag(&board, Point::new(150.0, 150.0), Point::new(250.0, 190.0));

    let scene = board.read_untracked();
    let held = scene.element(&Id::new("a")).expect("the shape");
    assert!((held.x - 200.0).abs() < 1e-6, "x is {}", held.x);
    assert!((held.y - 140.0).abs() < 1e-6, "y is {}", held.y);
    assert_eq!(board.revision.get_untracked(), 1);
    assert!(scene.is_selected(&Id::new("a")), "and it stays selected");
}

#[test]
fn a_band_takes_what_it_wholly_holds() {
    let (_window, board) = editor(
        r#"[{"type":"rectangle","id":"in","x":100,"y":100,"width":50,"height":50},
            {"type":"rectangle","id":"out","x":400,"y":100,"width":50,"height":50}]"#,
    );
    drag(&board, Point::new(50.0, 50.0), Point::new(300.0, 300.0));

    let scene = board.read_untracked();
    assert_eq!(scene.selection(), [Id::new("in")]);
    assert_eq!(
        board.revision.get_untracked(),
        0,
        "selecting is not an edit"
    );
}

#[test]
fn a_pen_stroke_is_stored_at_half_the_width_of_a_shape() {
    let (_window, board) = editor("[]");
    board.tool.set(Tool::Freedraw);
    let held = zdt_excalidraw::pointer::Held::default();
    zdt_excalidraw::pointer::down(&board, Point::new(100.0, 100.0), held);
    zdt_excalidraw::pointer::moved(&board, Point::new(160.0, 120.0), held);
    zdt_excalidraw::pointer::up(&board);

    board.tool.set(Tool::Rectangle);
    drag(&board, Point::new(300.0, 300.0), Point::new(400.0, 380.0));

    let scene = board.read_untracked();
    let stroke = scene
        .elements()
        .iter()
        .find(|held| held.kind == Kind::Freedraw)
        .expect("the stroke");
    let shape = scene
        .elements()
        .iter()
        .find(|held| held.kind == Kind::Rectangle)
        .expect("the shape");
    assert!(
        (stroke.stroke_width * 2.0 - shape.stroke_width).abs() < 1e-9,
        "the stroke is {} against the shape's {}",
        stroke.stroke_width,
        shape.stroke_width
    );
}

#[test]
fn drawing_by_hand_leaves_one_stroke() {
    let (_window, board) = editor("[]");
    board.tool.set(Tool::Freedraw);
    let held = zdt_excalidraw::pointer::Held::default();
    zdt_excalidraw::pointer::down(&board, Point::new(100.0, 100.0), held);
    for step in 1..10 {
        let at = Point::new(
            100.0 + f64::from(step) * 10.0,
            100.0 + f64::from(step) * 4.0,
        );
        zdt_excalidraw::pointer::moved(&board, at, held);
    }
    zdt_excalidraw::pointer::up(&board);

    let scene = board.read_untracked();
    assert_eq!(scene.elements().len(), 1);
    let stroke = scene.elements()[0].freedraw().expect("a stroke");
    assert!(stroke.points.len() >= 10, "every point was kept");
    assert_eq!(board.revision.get_untracked(), 1);
}

#[test]
fn dragging_a_line_open_finishes_it_and_chooses_it() {
    let (_window, board) = editor("[]");
    board.tool.set(Tool::Arrow);
    drag(&board, Point::new(100.0, 100.0), Point::new(300.0, 180.0));

    let scene = board.read_untracked();
    assert_eq!(scene.elements().len(), 1, "one arrow, drawn and finished");
    let arrow = scene.elements()[0].linear().expect("an arrow");
    assert_eq!(arrow.points.len(), 2, "and it is two points, not a walk");
    assert!(scene.has_selection(), "and it is chosen");
}

#[test]
fn a_chosen_line_offers_a_point_between_each_pair_of_its_own() {
    let (_window, board) =
        editor(r#"[{"type":"line","id":"a","x":100,"y":100,"points":[[0,0],[200,0]]}]"#);
    board.with_scene(|scene| scene.select([Id::new("a")]));

    let scene = board.read_untracked();
    let element = scene.element(&Id::new("a")).expect("the line").clone();
    drop(scene);
    let handles = zdt_excalidraw::handles::point_handles(&element, &board.viewport);
    assert_eq!(handles.len(), 3, "two points and the middle between them");
    assert_eq!(handles.iter().filter(|held| held.real).count(), 2);
    let middle = handles.iter().find(|held| !held.real).expect("the middle");
    assert!((middle.at.x - 200.0).abs() < 1e-6, "halfway along");
}

#[test]
fn dragging_the_middle_of_a_line_bends_it_and_leaves_two_more_middles() {
    let (_window, board) =
        editor(r#"[{"type":"line","id":"a","x":100,"y":100,"points":[[0,0],[200,0]]}]"#);
    board.with_scene(|scene| scene.select([Id::new("a")]));

    let scene = board.read_untracked();
    let element = scene.element(&Id::new("a")).expect("the line").clone();
    drop(scene);
    let middle = zdt_excalidraw::handles::point_handles(&element, &board.viewport)
        .into_iter()
        .find(|held| !held.real)
        .expect("the middle");

    let held = zdt_excalidraw::pointer::Held::default();
    assert!(zdt_excalidraw::pointer::down(&board, middle.at, held));
    zdt_excalidraw::pointer::moved(&board, Point::new(middle.at.x, middle.at.y - 80.0), held);
    zdt_excalidraw::pointer::up(&board);

    let scene = board.read_untracked();
    let line = scene.element(&Id::new("a")).expect("the line");
    let points = &line.linear().expect("a line").points;
    assert_eq!(points.len(), 3, "the middle became a point of its own");
    // And it went where it was dragged.
    let bend = excalidraw::geom::Placement::of(line).scene(points[1]);
    assert!(
        (bend.y - (middle.at.y - 80.0)).abs() < 1.0,
        "bend at {bend:?}"
    );

    // Which leaves a middle in each half.
    let now = zdt_excalidraw::handles::point_handles(line, &board.viewport);
    assert_eq!(now.iter().filter(|held| !held.real).count(), 2);
}

#[test]
fn walking_a_run_of_points_makes_one_arrow() {
    let (_window, board) = editor("[]");
    board.tool.set(Tool::Arrow);
    let held = zdt_excalidraw::pointer::Held::default();
    zdt_excalidraw::pointer::down(&board, Point::new(100.0, 100.0), held);
    zdt_excalidraw::pointer::up(&board);
    zdt_excalidraw::pointer::add_point(&board, Point::new(200.0, 60.0));
    zdt_excalidraw::pointer::add_point(&board, Point::new(300.0, 140.0));
    assert!(
        board.read_untracked().elements().is_empty(),
        "nothing is written until the run is finished"
    );
    zdt_excalidraw::pointer::finish_points(&board);

    let scene = board.read_untracked();
    assert_eq!(scene.elements().len(), 1);
    let arrow = scene.elements()[0].linear().expect("an arrow");
    assert_eq!(arrow.points.len(), 3);
    assert_eq!(board.revision.get_untracked(), 1);
}

#[test]
fn a_shape_is_chosen_once_it_is_drawn_and_a_pen_stroke_is_not() {
    let (_window, board) = editor("[]");

    board.tool.set(Tool::Rectangle);
    drag(&board, Point::new(100.0, 100.0), Point::new(200.0, 180.0));
    assert!(
        board.read_untracked().has_selection(),
        "a shape is chosen, so it can be coloured or moved straight away"
    );

    board.tool.set(Tool::Freedraw);
    let held = zdt_excalidraw::pointer::Held::default();
    zdt_excalidraw::pointer::down(&board, Point::new(300.0, 300.0), held);
    zdt_excalidraw::pointer::moved(&board, Point::new(360.0, 330.0), held);
    zdt_excalidraw::pointer::up(&board);
    assert!(
        !board.read_untracked().has_selection(),
        "a pen stroke is not: drawing is a run of them"
    );
}

#[test]
fn the_eraser_marks_what_it_will_take_before_it_takes_it() {
    let (_window, board) = editor(
        r##"[{"type":"rectangle","id":"a","x":100,"y":100,"width":100,"height":100,
              "backgroundColor":"#a5d8ff"}]"##,
    );
    board.tool.set(Tool::Eraser);
    let held = zdt_excalidraw::pointer::Held::default();
    zdt_excalidraw::pointer::down(&board, Point::new(90.0, 150.0), held);
    zdt_excalidraw::pointer::moved(&board, Point::new(150.0, 150.0), held);

    // Marked and faded, but still there: letting go is what takes it away.
    assert!(
        board.fade(&Id::new("a")) < 1.0,
        "it is drawn faintly while the pointer is down"
    );
    assert!(
        !board
            .read_untracked()
            .element(&Id::new("a"))
            .expect("it")
            .is_deleted
    );

    zdt_excalidraw::pointer::up(&board);
    assert!(
        board
            .read_untracked()
            .element(&Id::new("a"))
            .expect("it")
            .is_deleted
    );
    assert!(
        (board.fade(&Id::new("a")) - 1.0).abs() < f64::EPSILON,
        "and nothing is left marked"
    );
}

/// The eraser asks the expensive question only where a box says it might be worth asking, so the
/// boxes have to hold the ink — which for a pen stroke spreads well past the points it was drawn
/// from.
#[test]
fn the_eraser_still_finds_a_stroke_it_passes_over() {
    let (_window, board) = editor(
        r#"[{"type":"freedraw","id":"a","x":100,"y":100,"points":[[0,0],[100,0],[200,0]],
             "strokeWidth":4,"simulatePressure":true}]"#,
    );
    board.tool.set(Tool::Eraser);

    let held = zdt_excalidraw::pointer::Held::default();
    zdt_excalidraw::pointer::down(&board, Point::new(60.0, 100.0), held);
    zdt_excalidraw::pointer::moved(&board, Point::new(200.0, 100.0), held);
    assert!(
        board.fade(&Id::new("a")) < 1.0,
        "the stroke is marked as the pointer crosses it"
    );

    zdt_excalidraw::pointer::up(&board);
    assert!(
        board
            .read_untracked()
            .element(&Id::new("a"))
            .expect("it")
            .is_deleted
    );
}

#[test]
fn giving_up_an_erase_leaves_everything_where_it_was() {
    let (_window, board) = editor(
        r##"[{"type":"rectangle","id":"a","x":100,"y":100,"width":100,"height":100,
              "backgroundColor":"#a5d8ff"}]"##,
    );
    board.tool.set(Tool::Eraser);
    let held = zdt_excalidraw::pointer::Held::default();
    zdt_excalidraw::pointer::down(&board, Point::new(90.0, 150.0), held);
    zdt_excalidraw::pointer::moved(&board, Point::new(150.0, 150.0), held);
    zdt_excalidraw::pointer::cancel(&board);

    assert!(
        !board
            .read_untracked()
            .element(&Id::new("a"))
            .expect("it")
            .is_deleted
    );
    assert!((board.fade(&Id::new("a")) - 1.0).abs() < f64::EPSILON);
}

#[test]
fn the_eraser_takes_away_what_it_passes_over() {
    let (_window, board) = editor(
        r##"[{"type":"rectangle","id":"a","x":100,"y":100,"width":100,"height":100,
              "backgroundColor":"#a5d8ff"}]"##,
    );
    board.tool.set(Tool::Eraser);
    drag(&board, Point::new(90.0, 150.0), Point::new(150.0, 150.0));

    let scene = board.read_untracked();
    assert!(
        scene
            .element(&Id::new("a"))
            .expect("still in the file")
            .is_deleted
    );
    assert_eq!(board.revision.get_untracked(), 1);
}

#[test]
fn what_the_editor_writes_reads_back_as_a_drawing() {
    let (_window, board) = editor("[]");
    board.tool.set(Tool::Ellipse);
    drag(&board, Point::new(10.0, 10.0), Point::new(110.0, 90.0));

    let text = board.read_untracked().to_string().expect("it writes");
    let read = excalidraw::file::parse(&text).expect("and reads");
    assert_eq!(read.elements.len(), 1);
    assert_eq!(read.elements[0].kind, Kind::Ellipse);
}

#[test]
fn an_arrow_drawn_onto_a_shape_is_fixed_to_it() {
    let (_window, board) = editor(
        r##"[{"type":"rectangle","id":"box","x":300,"y":100,"width":100,"height":100,
              "backgroundColor":"#a5d8ff"}]"##,
    );
    board.tool.set(Tool::Arrow);
    let held = zdt_excalidraw::pointer::Held::default();
    zdt_excalidraw::pointer::down(&board, Point::new(100.0, 150.0), held);
    zdt_excalidraw::pointer::up(&board);
    // The last point lands inside the shape.
    zdt_excalidraw::pointer::add_point(&board, Point::new(340.0, 150.0));
    zdt_excalidraw::pointer::finish_points(&board);

    let scene = board.read_untracked();
    let arrow = scene
        .elements()
        .iter()
        .find(|held| held.kind == Kind::Arrow)
        .expect("the arrow");
    let binding = arrow
        .linear()
        .expect("an arrow")
        .end_binding
        .as_ref()
        .expect("it was fixed to the shape");
    assert_eq!(binding.element.as_str(), "box");

    // And the shape now knows what is fixed to it.
    let shape = scene.element(&Id::new("box")).expect("the shape");
    assert_eq!(shape.bound_elements.len(), 1);
}

#[test]
fn moving_a_shape_drags_the_arrow_fixed_to_it() {
    let (_window, board) = editor(
        r##"[{"type":"rectangle","id":"box","x":300,"y":100,"width":100,"height":100,
              "backgroundColor":"#a5d8ff",
              "boundElements":[{"id":"arr","type":"arrow"}]},
             {"type":"arrow","id":"arr","x":100,"y":150,"points":[[0,0],[200,0]],
              "endBinding":{"elementId":"box","fixedPoint":[0,0.5001],"mode":"inside"}}]"##,
    );
    let before = board
        .read_untracked()
        .element(&Id::new("arr"))
        .expect("the arrow")
        .width;

    // Drag the shape by its middle, which is inside it because it is filled.
    drag(&board, Point::new(350.0, 150.0), Point::new(450.0, 150.0));

    let after = board
        .read_untracked()
        .element(&Id::new("arr"))
        .expect("the arrow")
        .width;
    assert!(
        after > before + 90.0,
        "the arrow stretched: {before} to {after}"
    );
}

#[test]
fn typing_puts_words_into_the_element_it_opened() {
    use zdt_excalidraw::text;

    let (_window, board) = editor("[]");
    text::open_at(&board, Point::new(200.0, 200.0));
    assert!(text::is_open(&board), "a press on nothing makes words");

    for letter in ["h", "e", "l", "l", "o"] {
        assert!(text::insert(&board, letter));
    }
    assert!(text::backspace(&board));
    assert!(text::newline(&board));
    assert!(text::insert(&board, "there"));
    assert!(text::finish(&board));
    assert!(!text::is_open(&board), "and finishing closes them");

    let scene = board.read_untracked();
    let words = scene
        .elements()
        .iter()
        .find(|held| held.kind == Kind::Text)
        .expect("the words")
        .text()
        .expect("text")
        .clone();
    assert_eq!(words.original_text, "hell\nthere");
    assert!(words.text.contains("there"));
}

#[test]
fn the_text_tool_places_words_with_one_press() {
    use zdt_excalidraw::text;

    let (_window, board) = editor("[]");
    board.tool.set(Tool::Text);
    let held = zdt_excalidraw::pointer::Held::default();
    assert!(zdt_excalidraw::pointer::down(
        &board,
        Point::new(300.0, 200.0),
        held
    ));
    zdt_excalidraw::pointer::up(&board);
    assert!(text::is_open(&board), "one press opened the words");

    text::insert(&board, "hi");
    // And a press somewhere else finishes them.
    zdt_excalidraw::pointer::down(&board, Point::new(600.0, 500.0), held);
    assert!(!text::is_open(&board));

    let scene = board.read_untracked();
    let words = scene
        .elements()
        .iter()
        .find(|held| held.kind == Kind::Text)
        .expect("the words");
    assert_eq!(words.text().expect("text").original_text, "hi");
}

#[test]
fn words_left_empty_are_taken_away_again() {
    use zdt_excalidraw::text;

    let (_window, board) = editor("[]");
    text::open_at(&board, Point::new(200.0, 200.0));
    assert!(text::finish(&board));

    let scene = board.read_untracked();
    assert!(
        scene.alive().all(|held| held.kind != Kind::Text),
        "a press that opened words by accident leaves none behind"
    );
}

#[test]
fn a_press_on_a_shape_writes_inside_it() {
    use zdt_excalidraw::text;

    let (_window, board) = editor(
        r##"[{"type":"rectangle","id":"box","x":100,"y":100,"width":200,"height":100,
              "backgroundColor":"#a5d8ff"}]"##,
    );
    text::open_at(&board, Point::new(200.0, 150.0));
    text::insert(&board, "in");
    text::finish(&board);

    let scene = board.read_untracked();
    let label = scene
        .elements()
        .iter()
        .find(|held| held.kind == Kind::Text)
        .expect("the label");
    assert_eq!(
        label
            .text()
            .expect("text")
            .container_id
            .as_ref()
            .map(Id::as_str),
        Some("box"),
        "the words went inside the shape"
    );
    let shape = scene.element(&Id::new("box")).expect("the shape");
    assert_eq!(shape.bound_text().map(Id::as_str), Some(label.id.as_str()));
}

#[test]
fn turning_the_selection_turns_it_where_it_is() {
    let (_window, board) = editor(
        r##"[{"type":"rectangle","id":"a","x":100,"y":100,"width":200,"height":100,
              "backgroundColor":"#a5d8ff"}]"##,
    );
    board.with_scene(|scene| scene.select([Id::new("a")]));

    // The middle of the shape, which is what it turns about.
    let before = board
        .read_untracked()
        .element(&Id::new("a"))
        .expect("it")
        .clone();
    let middle = excalidraw::geom::bounds::center(&before);

    let frame = zdt_excalidraw::pointer::frame_untracked(&board).expect("a frame");
    let handle = frame.rotation_handle();
    let held = zdt_excalidraw::pointer::Held::default();
    zdt_excalidraw::pointer::down(&board, handle, held);
    // A quarter turn clockwise about the middle.
    let quarter = Point::new(middle.x + 200.0, middle.y);
    zdt_excalidraw::pointer::moved(&board, quarter, held);
    zdt_excalidraw::pointer::up(&board);

    let after = board
        .read_untracked()
        .element(&Id::new("a"))
        .expect("it")
        .clone();
    assert!(after.angle.abs() > 1.0, "it turned: {}", after.angle);
    let moved = excalidraw::geom::bounds::center(&after);
    assert!(
        (moved - middle).hypot() < 1e-6,
        "and it turned where it was: {middle:?} to {moved:?}"
    );
}

/// Every node under `from`, depth first.
fn every_node(window: &Window, from: zgui::view::NodeId) -> Vec<zgui::view::NodeId> {
    let mut found = vec![from];
    for child in window.dom.tree().children(from) {
        found.extend(every_node(window, child));
    }
    found
}
