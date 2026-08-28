//! An element, as the shapes something paints.
//!
//! A caller asks for [`pieces`] and paints what comes back, in order. Every path is in the scene's
//! coordinates and shared, so an element that has not changed hands back the same allocation and a
//! rasteriser that recognises geometry by its allocation keeps its work.
//!
//! What a piece does not carry is a colour: the colours are the element's own strings, and turning
//! one into whatever a renderer paints with is the renderer's business.

mod cache;
mod paint;

use std::sync::Arc;

use excalidraw_rough::{Options, ops::OpSetKind, to_path};
use kurbo::{Affine, BezPath, Stroke};

use crate::element::{Data, Element, Kind, is_transparent};
use crate::geom::{Placement, arrowhead, elbow, outline};

pub use self::cache::Cache;
pub use self::paint::{Paint, Piece};

/// The shapes `element` is painted as, in order.
///
/// The paths are in the scene's coordinates, so a caller draws them where they are.
#[must_use]
pub fn pieces(element: &Element) -> Vec<Piece> {
    let mut pieces = local_pieces(element);
    let to_scene = Placement::of(element).to_scene();
    for piece in &mut pieces {
        let mut path = BezPath::clone(&piece.path);
        path.apply_affine(to_scene);
        piece.path = Arc::new(path);
    }
    pieces
}

/// The same, in the element's own space.
#[must_use]
pub fn local_pieces(element: &Element) -> Vec<Piece> {
    if element.is_deleted {
        return Vec::new();
    }
    match element.kind {
        Kind::Freedraw => freedraw(element),
        Kind::Line | Kind::Arrow => linear(element),
        Kind::Text | Kind::Image => Vec::new(),
        Kind::Frame | Kind::Magicframe => frame(element),
        Kind::Selection => Vec::new(),
        _ => boxed(element),
    }
}

/// The options `element` is drawn with.
///
/// This is the one place an element's fields become a drawing: everything a caller could get wrong
/// about roughness, dashes, hachure or fills is decided here.
#[must_use]
pub fn options(element: &Element, continuous: bool) -> Options {
    let dashed = element.stroke_style != crate::element::StrokeStyle::Solid;
    Options {
        seed: element.rough_seed(),
        // A dashed outline is drawn once and a little wider, because two strokes of dashes read as
        // a muddle rather than as a line.
        stroke_width: if dashed {
            element.stroke_width + 0.5
        } else {
            element.stroke_width
        },
        disable_multi_stroke: dashed,
        // Set here rather than left to the library, so widening the outline for dashes does not
        // also change the fill.
        fill_weight: element.stroke_width / 2.0,
        hachure_gap: element.stroke_width * 4.0,
        roughness: roughness(element),
        fill_style: element.fill_style.to_rough(),
        filled: element.is_filled(),
        // A curve that has to meet what comes next keeps its ends; so does anything but the
        // loosest hand.
        preserve_vertices: continuous
            || element.roughness < crate::element::style::roughness::CARTOONIST,
        ..Options::default()
    }
}

/// How rough `element` is actually drawn.
///
/// A small shape drawn as roughly as a large one is a scribble, so the hand steadies as the shape
/// shrinks.
fn roughness(element: &Element) -> f64 {
    let largest = element.width.max(element.height);
    let smallest = element.width.min(element.height);
    let steady = (smallest >= 20.0 && largest >= 50.0)
        || (smallest >= 15.0 && element.roundness.is_some() && element.kind.can_be_round())
        || (element.kind.is_linear() && largest >= 50.0);
    if steady {
        return element.roughness;
    }
    (element.roughness / if largest < 10.0 { 3.0 } else { 2.0 }).min(2.5)
}

/// The dashes `element`'s outline is broken with.
#[must_use]
pub fn stroke(element: &Element) -> Stroke {
    let width = element.stroke_width;
    let stroke = Stroke::new(width)
        .with_caps(kurbo::Cap::Round)
        .with_join(kurbo::Join::Round);
    match element.stroke_style.dashes(width) {
        Some(dashes) => stroke.with_dashes(0.0, dashes),
        None => stroke,
    }
}

/// A rectangle, a diamond, an ellipse or a web page.
fn boxed(element: &Element) -> Vec<Piece> {
    let (width, height) = (element.width, element.height);
    if width <= 0.0 && height <= 0.0 {
        return Vec::new();
    }
    let rounded = outline::corner_radius(element, width.min(height)) > 0.0;
    let options = options(element, rounded);
    let mut random = options.random();

    let drawn = match element.kind {
        Kind::Ellipse => excalidraw_rough::shape::ellipse(
            kurbo::Point::new(width / 2.0, height / 2.0),
            width,
            height,
            &options,
            &mut random,
        ),
        Kind::Diamond if !rounded => {
            let points = crate::geom::diamond_points(width, height);
            excalidraw_rough::shape::polygon(&points, true, &options, &mut random)
        }
        Kind::Rectangle | Kind::Embeddable | Kind::Iframe if !rounded => {
            excalidraw_rough::shape::rectangle(0.0, 0.0, width, height, &options, &mut random)
        }
        // A rounded shape is drawn from its own outline, because the drawing library has no
        // notion of a cut corner.
        _ => excalidraw_rough::shape::path(&outline::of(element).to_svg(), &options, &mut random),
    };
    paint::of_drawable(&drawn, element)
}

/// A line or an arrow, and the heads on its ends.
fn linear(element: &Element) -> Vec<Piece> {
    let Data::Linear(linear) = &element.data else {
        return Vec::new();
    };
    if linear.points.len() < 2 {
        return Vec::new();
    }
    let round = element.roundness.is_some() && linear.points.len() > 2;
    let options = options(element, linear.elbowed);
    let mut random = options.random();

    let drawn = if linear.elbowed {
        let svg = elbow::as_svg(&linear.points, elbow::CORNER_RADIUS);
        excalidraw_rough::shape::path(&svg, &options, &mut random)
    } else if round {
        excalidraw_rough::shape::curve(&linear.points, &options, &mut random)
    } else if linear.polygon && element.is_filled() {
        excalidraw_rough::shape::polygon(&linear.points, true, &options, &mut random)
    } else {
        excalidraw_rough::shape::linear_path(&linear.points, &options, &mut random)
    };

    let mut pieces = paint::of_drawable(&drawn, element);

    // The heads are aimed along the line as it was drawn, so a round line's head follows the curve
    // rather than the points behind it.
    let Some(set) = drawn.outline() else {
        return pieces;
    };
    let path = to_path::of_ops(&set.ops);
    let last =
        |from: usize, to: usize| -> f64 { (linear.points[to] - linear.points[from]).hypot() };
    let count = linear.points.len();
    for (end, head) in [
        (arrowhead::End::Start, linear.start_arrowhead),
        (arrowhead::End::End, linear.end_arrowhead),
    ] {
        let Some(head) = head else { continue };
        let segment = match end {
            arrowhead::End::Start => last(0, 1),
            arrowhead::End::End => last(count - 2, count - 1),
        };
        let Some(at) = arrowhead::geometry(&path, end, head, segment, 0.0) else {
            continue;
        };
        pieces.extend(paint::of_arrowhead(head, &at, element));
    }
    pieces
}

/// A pen stroke, which is a filled ring rather than a stroked line.
fn freedraw(element: &Element) -> Vec<Piece> {
    let Data::Freedraw(stroke) = &element.data else {
        return Vec::new();
    };
    if stroke.points.is_empty() {
        return Vec::new();
    }
    let ring = excalidraw_rough::Stroke {
        points: &stroke.points,
        pressures: &stroke.pressures,
        simulate_pressure: stroke.simulate_pressure,
        stroke_width: element.stroke_width,
        streamline: stroke.streamline,
        variability: stroke.variability,
    }
    .path();

    let mut pieces = Vec::new();
    // A stroke that comes back to where it started has an inside, and the element's background
    // fills it.
    if element.is_filled() && is_a_loop(&stroke.points) {
        let options = options(element, false);
        let mut random = options.random();
        let under = excalidraw_rough::shape::curve(&stroke.points, &options, &mut random);
        pieces.extend(
            paint::of_drawable(&under, element)
                .into_iter()
                .filter(|piece| piece.fill.is_some()),
        );
    }
    pieces.push(Piece {
        path: Arc::new(ring),
        fill: Some(Paint {
            color: element.stroke_color.clone(),
            alpha: element.alpha(),
        }),
        stroke: None,
        even_odd: false,
    });
    pieces
}

/// A named box, which is drawn in the frame's own grey rather than the element's colours.
fn frame(element: &Element) -> Vec<Piece> {
    let box_ = kurbo::RoundedRect::new(0.0, 0.0, element.width, element.height, FRAME_RADIUS);
    vec![Piece {
        path: Arc::new(kurbo::Shape::to_path(&box_, 0.1)),
        fill: None,
        stroke: Some((
            Paint {
                color: frame_color(element.kind).to_owned(),
                alpha: element.alpha(),
            },
            Stroke::new(FRAME_STROKE_WIDTH),
        )),
        even_odd: false,
    }]
}

/// How large a frame's corners are cut.
pub const FRAME_RADIUS: f64 = 8.0;
/// How wide its outline is.
pub const FRAME_STROKE_WIDTH: f64 = 2.0;
/// How large its name is drawn.
pub const FRAME_NAME_FONT_SIZE: f64 = 14.0;
/// How far above the frame its name sits.
pub const FRAME_NAME_OFFSET: f64 = 3.0;

/// What a frame's outline is drawn in.
#[must_use]
pub const fn frame_color(kind: Kind) -> &'static str {
    match kind {
        Kind::Magicframe => "#7affd7",
        _ => "#bbbbbb",
    }
}

/// Whether a run of points comes back to where it started.
fn is_a_loop(points: &[kurbo::Point]) -> bool {
    let (Some(first), Some(last)) = (points.first(), points.last()) else {
        return false;
    };
    if points.len() < 3 {
        return false;
    }
    // Within a tenth of the run's own size, which is what Excalidraw treats as closed.
    let reach = points
        .iter()
        .map(|point| (*point - *first).hypot())
        .fold(0.0_f64, f64::max);
    (*last - *first).hypot() <= reach * 0.1
}

/// The colour a shape's inside is filled with, when it has one.
#[must_use]
pub fn fill_color(element: &Element) -> Option<&str> {
    (!is_transparent(&element.background_color)).then_some(element.background_color.as_str())
}

/// Whether a drawn set fills or strokes.
#[must_use]
pub const fn is_fill(kind: OpSetKind) -> bool {
    matches!(kind, OpSetKind::FillPath | OpSetKind::FillSketch)
}

/// The transform from an element's own space to the scene's.
#[must_use]
pub fn to_scene(element: &Element) -> Affine {
    Placement::of(element).to_scene()
}
