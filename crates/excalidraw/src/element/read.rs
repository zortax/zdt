//! Reading one element out of the object a file holds.
//!
//! Every field has a default, and several have a migration behind them, because a drawing may have
//! been written by any version of Excalidraw since the format existed. Nothing here fails: a field
//! that cannot be read takes its default, so one strange element does not cost the reader the whole
//! drawing.

use kurbo::Point;
use serde_json::{Map, Value};

use super::font::DEFAULT_FONT_SIZE;
use super::style::{ADAPTIVE_RADIUS, Arrowhead, FillStyle, Roundness, StrokeStyle};
use super::{
    BindMode, Binding, Bound, BoundKind, Crop, Data, Element, FixedSegment, FontFamily, Frame,
    Freedraw, Id, Image, Kind, Linear, Seed, Text, TextAlign, VerticalAlign, flag, number, string,
};

/// The colour an outline is drawn in when a file does not say.
pub const DEFAULT_STROKE_COLOR: &str = "#1e1e1e";
/// The colour an inside is filled with. Nothing.
pub const DEFAULT_BACKGROUND_COLOR: &str = "transparent";
/// How wide an outline is.
pub const DEFAULT_STROKE_WIDTH: f64 = 2.0;
/// How far the hand wanders.
pub const DEFAULT_ROUGHNESS: f64 = 1.0;
/// How solid an element is.
pub const DEFAULT_OPACITY: f64 = 100.0;
/// How much a pen stroke is pulled towards a smooth line.
pub const DEFAULT_STREAMLINE: f64 = 0.5;

/// The largest a linear element may be before it is treated as broken.
const MAX_LINEAR: f64 = 75_000.0;

/// The element `object` describes, when it describes one.
///
/// A `selection` element answers nothing: it is the rubber band of a session that has ended, and
/// Excalidraw drops it on load too.
#[must_use]
pub fn element(object: &Map<String, Value>) -> Option<Element> {
    let kind = kind(object)?;
    if kind == Kind::Selection {
        return None;
    }

    let mut element = Element {
        id: string(object, "id").map_or_else(|| Id::new(String::new()), Id::new),
        kind,
        x: number(object, "x").unwrap_or(0.0),
        y: number(object, "y").unwrap_or(0.0),
        width: number(object, "width").unwrap_or(0.0),
        height: number(object, "height").unwrap_or(0.0),
        angle: number(object, "angle").unwrap_or(0.0),
        stroke_color: string(object, "strokeColor")
            .unwrap_or_else(|| DEFAULT_STROKE_COLOR.to_owned()),
        background_color: string(object, "backgroundColor")
            .unwrap_or_else(|| DEFAULT_BACKGROUND_COLOR.to_owned()),
        fill_style: word(object, "fillStyle").unwrap_or_default(),
        stroke_width: number(object, "strokeWidth")
            .filter(|width| *width > 0.0)
            .unwrap_or(DEFAULT_STROKE_WIDTH),
        stroke_style: word(object, "strokeStyle").unwrap_or_default(),
        roughness: number(object, "roughness").unwrap_or(DEFAULT_ROUGHNESS),
        opacity: number(object, "opacity").unwrap_or(DEFAULT_OPACITY),
        group_ids: strings(object, "groupIds"),
        frame_id: string(object, "frameId").map(Id::new),
        roundness: roundness(object, kind),
        seed: number(object, "seed").map_or(Seed(1), Seed::from_number),
        version: number(object, "version")
            .map_or(1, |held| held as u64)
            .max(1),
        version_nonce: number(object, "versionNonce").map_or(0, |held| held as u64),
        index: string(object, "index"),
        is_deleted: flag(object, "isDeleted").unwrap_or(false),
        bound_elements: bound_elements(object),
        updated: number(object, "updated").map_or(0, |held| held as u64),
        link: string(object, "link"),
        locked: flag(object, "locked").unwrap_or(false),
        data: Data::Shape,
    };

    // A negative size is the same box, measured from the other corner.
    if element.width < 0.0 {
        element.width = -element.width;
        element.x -= element.width;
    }
    if element.height < 0.0 {
        element.height = -element.height;
        element.y -= element.height;
    }

    element.data = match kind {
        Kind::Line | Kind::Arrow => Data::Linear(linear(object, kind, &mut element)),
        Kind::Freedraw => Data::Freedraw(freedraw(object)),
        Kind::Text => Data::Text(text(object, element.height)),
        Kind::Image => Data::Image(image(object)),
        Kind::Frame | Kind::Magicframe => Data::Frame(Frame {
            name: string(object, "name"),
        }),
        Kind::Embeddable | Kind::Iframe => Data::Embed,
        _ => Data::Shape,
    };
    Some(element)
}

/// Which kind `object` says it is.
fn kind(object: &Map<String, Value>) -> Option<Kind> {
    serde_json::from_value(object.get("type")?.clone()).ok()
}

/// A word out of `object`, read as `T`.
fn word<T: serde::de::DeserializeOwned>(object: &Map<String, Value>, key: &str) -> Option<T> {
    serde_json::from_value(object.get(key)?.clone()).ok()
}

/// A list of strings out of `object`.
fn strings(object: &Map<String, Value>, key: &str) -> Vec<String> {
    object
        .get(key)
        .and_then(Value::as_array)
        .map(|held| {
            held.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// How `object`'s corners are cut.
fn roundness(object: &Map<String, Value>, kind: Kind) -> Option<Roundness> {
    if let Some(held) = object.get("roundness").and_then(Value::as_object) {
        let value = number(held, "value");
        return match number(held, "type")? as i64 {
            1 => Some(Roundness::Legacy),
            2 => Some(Roundness::Proportional),
            3 => Some(Roundness::Adaptive {
                value: value.filter(|held| *held > 0.0),
            }),
            _ => None,
        };
    }
    // An old file said only that the corners were round. A shape that would now be cut to a fixed
    // radius keeps the proportion it was drawn with.
    match string(object, "strokeSharpness")?.as_str() {
        "round" if kind.adaptive_radius() => Some(Roundness::Legacy),
        "round" => Some(Roundness::Proportional),
        _ => None,
    }
}

/// What is bound to `object`.
fn bound_elements(object: &Map<String, Value>) -> Vec<Bound> {
    // The oldest files held a list of arrow ids and nothing else.
    if let Some(ids) = object.get("boundElementIds").and_then(Value::as_array) {
        return ids
            .iter()
            .filter_map(Value::as_str)
            .map(|id| Bound {
                id: Id::new(id),
                kind: BoundKind::Arrow,
            })
            .collect();
    }
    object
        .get("boundElements")
        .and_then(Value::as_array)
        .map(|held| {
            held.iter()
                .filter_map(Value::as_object)
                .filter_map(|bound| {
                    let id = Id::new(bound.get("id")?.as_str()?);
                    let kind = match bound.get("type")?.as_str()? {
                        "text" => BoundKind::Text,
                        "arrow" => BoundKind::Arrow,
                        _ => return None,
                    };
                    Some(Bound { id, kind })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The points `object` holds under `key`, in the element's own coordinates.
fn points(object: &Map<String, Value>, key: &str) -> Vec<Point> {
    object
        .get(key)
        .and_then(Value::as_array)
        .map(|held| {
            held.iter()
                .filter_map(|point| {
                    let pair = point.as_array()?;
                    Some(Point::new(pair.first()?.as_f64()?, pair.get(1)?.as_f64()?))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// A line or an arrow, with its points normalised onto its origin.
fn linear(object: &Map<String, Value>, kind: Kind, element: &mut Element) -> Linear {
    let mut points = points(object, "points");
    if points.len() < 2 {
        points = vec![Point::ZERO, Point::new(element.width, element.height)];
    }
    // The first point is the element's own origin, so whatever it was is folded into x and y.
    let first = points[0];
    if first != Point::ZERO {
        element.x += first.x;
        element.y += first.y;
        for point in &mut points {
            *point -= first.to_vec2();
        }
    }
    // The box is whatever the points need, which is what every bound and hit test is measured
    // against.
    let (mut min_x, mut max_x, mut min_y, mut max_y) = (0.0_f64, 0.0_f64, 0.0_f64, 0.0_f64);
    for point in &points {
        min_x = min_x.min(point.x);
        max_x = max_x.max(point.x);
        min_y = min_y.min(point.y);
        max_y = max_y.max(point.y);
    }
    element.width = max_x - min_x;
    element.height = max_y - min_y;

    // A line this large is a file that has gone wrong, not a drawing. Excalidraw removes it.
    if element.width > MAX_LINEAR || element.height > MAX_LINEAR {
        element.is_deleted = true;
        element.width = 100.0;
        element.height = 100.0;
        points = vec![Point::ZERO, Point::new(100.0, 100.0)];
    }

    let arrow = kind == Kind::Arrow;
    let elbowed = arrow && flag(object, "elbowed").unwrap_or(false);
    let fixed_segments = if elbowed {
        fixed_segments(object)
    } else {
        Vec::new()
    };

    Linear {
        // A line cannot bind: only an arrow's ends follow a shape.
        start_binding: arrow.then(|| binding(object, "startBinding")).flatten(),
        end_binding: arrow.then(|| binding(object, "endBinding")).flatten(),
        start_arrowhead: arrowhead(object, "startArrowhead", false),
        // An arrow with no word for its end has one: that is what an arrow is.
        end_arrowhead: arrowhead(object, "endArrowhead", arrow),
        elbowed,
        // Segments are only fixed once there are enough points to hold them.
        fixed_segments: if points.len() >= 4 {
            fixed_segments
        } else {
            Vec::new()
        },
        start_is_special: flag(object, "startIsSpecial").unwrap_or(false),
        end_is_special: flag(object, "endIsSpecial").unwrap_or(false),
        polygon: kind == Kind::Line && flag(object, "polygon").unwrap_or(false),
        points,
    }
}

/// The head at one end of a line.
///
/// `implied` is what an absent key means. An arrow with no word for its end is drawn with one; an
/// explicit `null` is drawn with none.
fn arrowhead(object: &Map<String, Value>, key: &str, implied: bool) -> Option<Arrowhead> {
    match object.get(key) {
        None => implied.then_some(Arrowhead::Arrow),
        Some(Value::Null) => None,
        Some(value) => value.as_str().and_then(Arrowhead::parse),
    }
}

/// Where one end of an arrow is fixed.
fn binding(object: &Map<String, Value>, key: &str) -> Option<Binding> {
    let held = object.get(key)?.as_object()?;
    let element = Id::new(held.get("elementId")?.as_str()?);
    let fixed_point = held
        .get("fixedPoint")
        .and_then(Value::as_array)
        .and_then(|pair| Some((pair.first()?.as_f64()?, pair.get(1)?.as_f64()?)))
        // An older binding said how far along the shape's diagonal the arrow pointed. Without the
        // shape here to measure, the middle is the honest answer, and a drag re-fixes it.
        .unwrap_or((0.5, 0.5));
    let mode = match string(held, "mode").as_deref() {
        Some("inside") => BindMode::Inside,
        Some("skip") => BindMode::Skip,
        _ => BindMode::Orbit,
    };
    Some(Binding {
        element,
        fixed_point: normalized_fixed_point(fixed_point),
        mode,
    })
}

/// A fixed point kept inside the range a shape can be measured over.
///
/// Exactly half is nudged off, because a point on the middle line flips which side of the shape the
/// arrow leaves from every time the shape is nudged.
fn normalized_fixed_point((x, y): (f64, f64)) -> (f64, f64) {
    let one = |value: f64| {
        let value = if value.is_finite() {
            value.clamp(-10.0, 10.0)
        } else {
            0.5001
        };
        if (value - 0.5).abs() < 1e-4 {
            0.5001
        } else {
            value
        }
    };
    (one(x), one(y))
}

/// The runs of an elbowed arrow the reader has fixed.
fn fixed_segments(object: &Map<String, Value>) -> Vec<FixedSegment> {
    object
        .get("fixedSegments")
        .and_then(Value::as_array)
        .map(|held| {
            held.iter()
                .filter_map(Value::as_object)
                .filter_map(|segment| {
                    let pair = |key: &str| -> Option<Point> {
                        let held = segment.get(key)?.as_array()?;
                        Some(Point::new(held.first()?.as_f64()?, held.get(1)?.as_f64()?))
                    };
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    Some(FixedSegment {
                        start: pair("start")?,
                        end: pair("end")?,
                        index: number(segment, "index")? as usize,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// A pen stroke.
fn freedraw(object: &Map<String, Value>) -> Freedraw {
    let points = points(object, "points");
    let pressures = object
        .get("pressures")
        .and_then(Value::as_array)
        .map(|held| {
            held.iter()
                .map(|value| {
                    value
                        .as_f64()
                        .filter(|held| held.is_finite())
                        // A pressure the device did not report is halfway.
                        .unwrap_or(0.5)
                })
                .collect()
        })
        .unwrap_or_default();

    let options = object.get("strokeOptions").and_then(Value::as_object);
    let variability = match options
        .and_then(|held| string(held, "variability"))
        .as_deref()
    {
        Some("constant") => excalidraw_rough::Variability::Constant,
        _ => excalidraw_rough::Variability::Variable,
    };
    let streamline = options
        .and_then(|held| number(held, "streamline"))
        .unwrap_or(DEFAULT_STREAMLINE);

    Freedraw {
        points,
        pressures,
        simulate_pressure: flag(object, "simulatePressure").unwrap_or(true),
        streamline,
        variability,
    }
}

/// Written words.
fn text(object: &Map<String, Value>, height: f64) -> Text {
    let body = string(object, "text").unwrap_or_default();
    let font_family = number(object, "fontFamily")
        .map(|held| FontFamily::from_number(held as u32))
        .unwrap_or_default();
    let font_size = number(object, "fontSize")
        .filter(|size| *size > 0.0)
        .unwrap_or(DEFAULT_FONT_SIZE);

    // A file written before line height was stored has it in the height it wrote.
    let lines = body.lines().count().max(1) as f64;
    let line_height = number(object, "lineHeight")
        .filter(|held| *held > 0.0)
        .or_else(|| {
            (height > 0.0)
                .then(|| height / lines / font_size)
                .filter(|held| *held > 0.0)
        })
        .unwrap_or_else(|| font_family.line_height());

    Text {
        original_text: string(object, "originalText").unwrap_or_else(|| body.clone()),
        text: body,
        font_size,
        font_family,
        text_align: word(object, "textAlign").unwrap_or(TextAlign::Left),
        vertical_align: word(object, "verticalAlign").unwrap_or(VerticalAlign::Top),
        container_id: string(object, "containerId").map(Id::new),
        auto_resize: flag(object, "autoResize").unwrap_or(true),
        line_height,
    }
}

/// A picture.
fn image(object: &Map<String, Value>) -> Image {
    let scale = object
        .get("scale")
        .and_then(Value::as_array)
        .and_then(|pair| Some((pair.first()?.as_f64()?, pair.get(1)?.as_f64()?)))
        .unwrap_or((1.0, 1.0));
    Image {
        file_id: string(object, "fileId"),
        status: word(object, "status").unwrap_or_default(),
        scale,
        crop: object
            .get("crop")
            .and_then(|held| serde_json::from_value::<Crop>(held.clone()).ok()),
    }
}

/// The radius a shape's corners are cut to when a file says only that they are round.
pub const DEFAULT_ROUND_RADIUS: f64 = ADAPTIVE_RADIUS;

/// The fill a shape gets when a file names none.
pub const DEFAULT_FILL_STYLE: FillStyle = FillStyle::Solid;

/// The outline it gets.
pub const DEFAULT_STROKE_STYLE: StrokeStyle = StrokeStyle::Solid;

#[cfg(test)]
mod tests {
    use super::*;

    fn read(json: &str) -> Option<Element> {
        let value: Value = serde_json::from_str(json).expect("valid JSON");
        element(value.as_object().expect("an object"))
    }

    #[test]
    fn an_element_with_almost_nothing_still_reads() {
        let held = read(r#"{"type":"rectangle"}"#).expect("a rectangle");
        assert_eq!(held.kind, Kind::Rectangle);
        assert_eq!(held.stroke_color, DEFAULT_STROKE_COLOR);
        assert_eq!(held.fill_style, FillStyle::Solid);
        assert!((held.stroke_width - 2.0).abs() < f64::EPSILON);
        assert_eq!(held.seed, Seed(1));
        assert_eq!(held.version, 1);
    }

    #[test]
    fn a_selection_is_not_an_element() {
        assert!(read(r#"{"type":"selection","x":0,"y":0}"#).is_none());
        assert!(read(r#"{"type":"nonsense"}"#).is_none());
    }

    #[test]
    fn a_negative_size_is_the_same_box_from_the_other_corner() {
        let held = read(r#"{"type":"rectangle","x":100,"y":50,"width":-40,"height":-20}"#)
            .expect("a rectangle");
        assert!((held.x - 60.0).abs() < f64::EPSILON);
        assert!((held.y - 30.0).abs() < f64::EPSILON);
        assert!((held.width - 40.0).abs() < f64::EPSILON);
        assert!((held.height - 20.0).abs() < f64::EPSILON);
    }

    #[test]
    fn an_arrow_with_no_word_for_its_end_still_has_a_head() {
        let arrow = read(r#"{"type":"arrow","points":[[0,0],[10,0]]}"#).expect("an arrow");
        assert_eq!(
            arrow.linear().expect("linear").end_arrowhead,
            Some(Arrowhead::Arrow)
        );

        let bare = read(r#"{"type":"arrow","points":[[0,0],[10,0]],"endArrowhead":null}"#)
            .expect("an arrow");
        assert_eq!(bare.linear().expect("linear").end_arrowhead, None);

        let line = read(r#"{"type":"line","points":[[0,0],[10,0]]}"#).expect("a line");
        assert_eq!(line.linear().expect("linear").end_arrowhead, None);
    }

    #[test]
    fn a_lines_first_point_becomes_its_origin() {
        let held =
            read(r#"{"type":"line","x":10,"y":20,"points":[[5,5],[25,15]]}"#).expect("a line");
        let linear = held.linear().expect("linear");
        assert_eq!(linear.points[0], Point::ZERO);
        assert_eq!(linear.points[1], Point::new(20.0, 10.0));
        assert!((held.x - 15.0).abs() < f64::EPSILON);
        assert!((held.y - 25.0).abs() < f64::EPSILON);
        assert!((held.width - 20.0).abs() < f64::EPSILON);
        assert!((held.height - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn a_line_can_never_bind() {
        let held = read(
            r#"{"type":"line","points":[[0,0],[10,0]],
                "startBinding":{"elementId":"a","fixedPoint":[0.2,0.3],"mode":"orbit"}}"#,
        )
        .expect("a line");
        assert!(held.linear().expect("linear").start_binding.is_none());
    }

    #[test]
    fn a_binding_is_kept_away_from_the_middle_line() {
        let held = read(
            r#"{"type":"arrow","points":[[0,0],[10,0]],
                "startBinding":{"elementId":"a","fixedPoint":[0.5,0.5],"mode":"inside"}}"#,
        )
        .expect("an arrow");
        let linear = held.linear().expect("linear");
        let binding = linear.start_binding.as_ref().expect("bound");
        assert_eq!(binding.mode, BindMode::Inside);
        assert!((binding.fixed_point.0 - 0.5001).abs() < f64::EPSILON);
    }

    #[test]
    fn the_oldest_bound_arrows_still_read() {
        let held = read(r#"{"type":"rectangle","boundElementIds":["a","b"]}"#).expect("a shape");
        assert_eq!(held.bound_elements.len(), 2);
        assert_eq!(held.bound_elements[0].kind, BoundKind::Arrow);
    }

    #[test]
    fn an_old_round_shape_keeps_the_proportion_it_was_drawn_with() {
        let rectangle =
            read(r#"{"type":"rectangle","strokeSharpness":"round"}"#).expect("a rectangle");
        assert_eq!(rectangle.roundness, Some(Roundness::Legacy));
        let diamond = read(r#"{"type":"diamond","strokeSharpness":"round"}"#).expect("a diamond");
        assert_eq!(diamond.roundness, Some(Roundness::Proportional));
        let sharp = read(r#"{"type":"rectangle","strokeSharpness":"sharp"}"#).expect("a rectangle");
        assert_eq!(sharp.roundness, None);
    }

    #[test]
    fn a_text_without_a_line_height_takes_it_from_the_height_it_was_written_with() {
        let held =
            read(r#"{"type":"text","text":"hi","fontSize":20,"height":30}"#).expect("some words");
        let text = held.text().expect("text");
        assert!((text.line_height - 1.5).abs() < f64::EPSILON);
        assert_eq!(text.original_text, "hi");
    }

    #[test]
    fn an_enormous_line_is_treated_as_broken() {
        let held = read(r#"{"type":"line","points":[[0,0],[80000,0]]}"#).expect("a line");
        assert!(held.is_deleted);
        assert!((held.width - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn a_freehand_stroke_reads_its_pressures_and_its_options() {
        let held = read(
            r#"{"type":"freedraw","points":[[0,0],[5,5]],"pressures":[0.2,0.8],
                "simulatePressure":false,
                "strokeOptions":{"variability":"constant","streamline":0.2}}"#,
        )
        .expect("a stroke");
        let stroke = held.freedraw().expect("freedraw");
        assert_eq!(stroke.pressures, vec![0.2, 0.8]);
        assert!(!stroke.simulate_pressure);
        assert_eq!(stroke.variability, excalidraw_rough::Variability::Constant);
        assert!((stroke.streamline - 0.2).abs() < f64::EPSILON);
    }
}
