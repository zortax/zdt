//! A drawing this crate reads and writes back is the file it read.
//!
//! The corpus is written by `mkcorpus.mjs` in the same shape Excalidraw writes: two-space JSON,
//! element keys in the order its own reader leaves them, whole numbers without a point. It covers
//! every element kind, every fill and stroke style, bound text, groups, frames, elbow arrows,
//! bindings, images, a deleted element, and an old file carrying keys this crate has never heard
//! of.
//!
//! Byte-identity is the point: a drawing saved after nothing changed must not move a single line,
//! or every save of a file made elsewhere would be one enormous diff.

use excalidraw::element::{Arrowhead, BindMode, BoundKind, Kind};
use excalidraw::file;

/// Every file in the corpus, by name.
fn corpus() -> Vec<(&'static str, &'static str)> {
    vec![
        ("empty", include_str!("corpus/empty.excalidraw")),
        ("shapes", include_str!("corpus/shapes.excalidraw")),
        ("linear", include_str!("corpus/linear.excalidraw")),
        ("freedraw", include_str!("corpus/freedraw.excalidraw")),
        ("text", include_str!("corpus/text.excalidraw")),
        ("images", include_str!("corpus/images.excalidraw")),
        ("groups", include_str!("corpus/groups.excalidraw")),
        ("legacy", include_str!("corpus/legacy.excalidraw")),
    ]
}

#[test]
fn every_file_is_written_back_byte_for_byte() {
    for (name, text) in corpus() {
        let drawing = file::parse(text).unwrap_or_else(|error| panic!("{name}: {error}"));
        let written = drawing
            .to_string()
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(written, text, "{name} was not written back as it was read");
    }
}

#[test]
fn a_file_with_keys_this_crate_has_never_heard_of_keeps_them() {
    let text = include_str!("corpus/legacy.excalidraw");
    let drawing = file::parse(text).expect("a drawing");
    let held = drawing.store.element(0).expect("the one element");
    assert!(
        held.contains_key("somethingNewer"),
        "the key survived the read"
    );
    assert_eq!(drawing.to_string().expect("it writes"), text);
}

#[test]
fn every_kind_in_the_corpus_reads() {
    let drawing = file::parse(include_str!("corpus/shapes.excalidraw")).expect("a drawing");
    let kinds: Vec<Kind> = drawing.elements.iter().map(|held| held.kind).collect();
    assert_eq!(
        kinds,
        [
            Kind::Rectangle,
            Kind::Rectangle,
            Kind::Diamond,
            Kind::Ellipse
        ]
    );

    let linear = file::parse(include_str!("corpus/linear.excalidraw")).expect("a drawing");
    assert_eq!(linear.elements.len(), 3);
    let arrow = linear.elements[1].linear().expect("an arrow");
    assert_eq!(arrow.start_arrowhead, Some(Arrowhead::Circle));
    assert_eq!(arrow.end_arrowhead, Some(Arrowhead::Triangle));
    let binding = arrow.start_binding.as_ref().expect("it is bound");
    assert_eq!(binding.element.as_str(), "rect-adaptive");
    assert_eq!(binding.mode, BindMode::Orbit);

    let elbow = linear.elements[2].linear().expect("an arrow");
    assert!(elbow.elbowed);
    assert_eq!(elbow.fixed_segments.len(), 1);
}

#[test]
fn a_bound_label_names_its_container_both_ways() {
    let drawing = file::parse(include_str!("corpus/text.excalidraw")).expect("a drawing");
    let (_, container) = drawing.find(&"boxed".into()).expect("the container");
    assert_eq!(
        container.bound_text().map(|id| id.as_str()),
        Some("inner"),
        "the container names its label"
    );
    assert_eq!(container.bound_elements[0].kind, BoundKind::Text);

    let (_, label) = drawing.find(&"inner".into()).expect("the label");
    let text = label.text().expect("words");
    assert_eq!(
        text.container_id.as_ref().map(excalidraw::Id::as_str),
        Some("boxed"),
        "the label names its container"
    );
    assert!(!text.auto_resize);
}

#[test]
fn a_frame_owns_the_elements_that_name_it() {
    let drawing = file::parse(include_str!("corpus/text.excalidraw")).expect("a drawing");
    let (_, frame) = drawing.find(&"frame1".into()).expect("the frame");
    assert_eq!(
        frame.frame().expect("a frame").name.as_deref(),
        Some("Overview")
    );
    let (_, label) = drawing.find(&"label1".into()).expect("the label");
    assert_eq!(
        label.frame_id.as_ref().map(excalidraw::Id::as_str),
        Some("frame1")
    );
}

#[test]
fn a_group_is_read_innermost_first() {
    let drawing = file::parse(include_str!("corpus/groups.excalidraw")).expect("a drawing");
    let (_, first) = drawing.find(&"g1".into()).expect("the first");
    assert_eq!(first.group_ids, ["inner-group", "outer-group"]);
    assert_eq!(first.outermost_group(), Some("outer-group"));
}

#[test]
fn a_deleted_element_is_read_and_kept() {
    let drawing = file::parse(include_str!("corpus/groups.excalidraw")).expect("a drawing");
    let (_, gone) = drawing
        .find(&"gone".into())
        .expect("it is still in the file");
    assert!(gone.is_deleted);
}

#[test]
fn a_pictures_bytes_come_back_out_of_the_file() {
    let drawing = file::parse(include_str!("corpus/images.excalidraw")).expect("a drawing");
    let image = drawing.elements[0].image().expect("a picture");
    let id = image.file_id.as_deref().expect("it names one");
    let held = drawing.files.get(id).expect("the file is there");
    assert_eq!(held.mime_type, "image/png");
    assert!(held.bytes().expect("it decodes").starts_with(b"\x89PNG"));
    // The negative scale is a flip down the page.
    assert!((image.scale.1 + 1.0).abs() < f64::EPSILON);
}

#[test]
fn a_library_file_reads_its_items() {
    let library = file::library::parse(include_str!("corpus/fixture_library.excalidrawlib"))
        .expect("a library");
    assert_eq!(library.items.len(), 1);
    assert_eq!(library.items[0].parsed.len(), 1);
    assert_eq!(library.items[0].parsed[0].kind, Kind::Rectangle);
}
