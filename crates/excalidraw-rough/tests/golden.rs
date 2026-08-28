//! What rough.js draws, drawn again here.
//!
//! `golden.json` holds the ops rough.js 4.6.4 answers for a set of shapes, captured by
//! `golden.mjs`. A stroke that wandered somewhere else, or that asked its generator for numbers in
//! another order, moves every number after it — so this one comparison covers the whole port.
//!
//! Regenerate with:
//!
//! ```text
//! npm install roughjs@4.6.4 && node tests/golden.mjs > tests/golden.json
//! ```
//!
//! One thing the numbers here caught that nothing else would: a solid fill is drawn by the same
//! generator as the outline it fills, carried on rather than started again, and its strokes are
//! merged into one ring before the even-odd rule decides what is inside.

use excalidraw_rough::ops::{Drawable, Op, OpSetKind};
use excalidraw_rough::options::FillStyle;
use excalidraw_rough::{Options, shape};
use kurbo::Point;
use serde_json::Value;

/// How far a number may be from the one JavaScript answered.
///
/// The captured values are rounded to six places, and the two run the same arithmetic in the same
/// order, so anything larger is a real difference.
const TOLERANCE: f64 = 5e-6;

/// The captured drawings.
fn golden() -> Value {
    serde_json::from_str(include_str!("golden.json")).expect("the capture is valid JSON")
}

/// The options every captured case was drawn with.
fn base() -> Options {
    Options {
        seed: 1_263_748_391,
        roughness: 1.0,
        bowing: 1.0,
        stroke_width: 2.0,
        ..Options::default()
    }
}

/// The options a filled case was drawn with.
fn filled(style: FillStyle) -> Options {
    Options {
        filled: true,
        fill_style: style,
        fill_weight: 1.0,
        hachure_gap: 8.0,
        ..base()
    }
}

/// What rough.js calls each kind of set.
fn kind_name(kind: OpSetKind) -> &'static str {
    match kind {
        OpSetKind::Path => "path",
        OpSetKind::FillPath => "fillPath",
        OpSetKind::FillSketch => "fillSketch",
    }
}

/// One op's numbers, in the order rough.js writes them.
fn numbers(op: &Op) -> Vec<f64> {
    match *op {
        Op::Move(at) | Op::Line(at) => vec![at.x, at.y],
        Op::Curve(c1, c2, at) => vec![c1.x, c1.y, c2.x, c2.y, at.x, at.y],
    }
}

/// What rough.js calls each op.
fn op_name(op: &Op) -> &'static str {
    match op {
        Op::Move(_) => "move",
        Op::Line(_) => "lineTo",
        Op::Curve(..) => "bcurveTo",
    }
}

/// Asserts that `drawn` is what rough.js drew for `case`.
fn matches(case: &str, drawn: &Drawable) {
    let golden = golden();
    let expected = golden
        .get(case)
        .unwrap_or_else(|| panic!("{case} is in the capture"))
        .as_array()
        .expect("a list of sets");

    assert_eq!(
        drawn.sets.len(),
        expected.len(),
        "{case}: {} sets drawn, rough.js drew {}",
        drawn.sets.len(),
        expected.len()
    );

    for (at, (set, expected)) in drawn.sets.iter().zip(expected).enumerate() {
        assert_eq!(
            kind_name(set.kind),
            expected["type"].as_str().expect("a kind"),
            "{case}: set {at} is the wrong kind"
        );
        let expected_ops = expected["ops"].as_array().expect("a list of ops");
        assert_eq!(
            set.ops.len(),
            expected_ops.len(),
            "{case}: set {at} has {} ops, rough.js drew {}",
            set.ops.len(),
            expected_ops.len()
        );
        for (index, (op, expected)) in set.ops.iter().zip(expected_ops).enumerate() {
            assert_eq!(
                op_name(op),
                expected["op"].as_str().expect("an op name"),
                "{case}: set {at} op {index} is the wrong op"
            );
            let drawn = numbers(op);
            let expected: Vec<f64> = expected["data"]
                .as_array()
                .expect("the numbers")
                .iter()
                .map(|value| value.as_f64().expect("a number"))
                .collect();
            assert_eq!(drawn.len(), expected.len());
            for (which, (drawn, expected)) in drawn.iter().zip(&expected).enumerate() {
                assert!(
                    (drawn - expected).abs() < TOLERANCE,
                    "{case}: set {at} op {index} number {which} is {drawn}, rough.js says {expected}"
                );
            }
        }
    }
}

/// The shape each captured case draws.
fn drawn(case: &str) -> Drawable {
    let diamond = [
        Point::new(110.0, 0.0),
        Point::new(220.0, 65.0),
        Point::new(110.0, 128.0),
        Point::new(0.0, 65.0),
    ];
    let bent = [
        Point::new(0.0, 0.0),
        Point::new(92.0, -26.0),
        Point::new(176.0, 16.0),
    ];
    let rounded = "M 32 0 L 188 0 Q 220 0, 220 32 L 220 96 Q 220 128, 188 128 \
                   L 32 128 Q 0 128, 0 96 L 0 32 Q 0 0, 32 0";

    /// A shape, drawn with the options its case names.
    type Draw = Box<dyn Fn(&Options, &mut excalidraw_rough::Random) -> Drawable>;

    let (options, draw): (Options, Draw) = match case {
        "rectangle_plain" => (
            base(),
            Box::new(|o, r| shape::rectangle(0.0, 0.0, 220.0, 128.0, o, r)),
        ),
        "rectangle_hachure" => (
            filled(FillStyle::Hachure),
            Box::new(|o, r| shape::rectangle(0.0, 0.0, 220.0, 128.0, o, r)),
        ),
        "rectangle_solid" => (
            filled(FillStyle::Solid),
            Box::new(|o, r| shape::rectangle(0.0, 0.0, 220.0, 128.0, o, r)),
        ),
        "rectangle_crosshatch" => (
            filled(FillStyle::CrossHatch),
            Box::new(|o, r| shape::rectangle(0.0, 0.0, 220.0, 128.0, o, r)),
        ),
        "rectangle_zigzag" => (
            filled(FillStyle::ZigZag),
            Box::new(|o, r| shape::rectangle(0.0, 0.0, 220.0, 128.0, o, r)),
        ),
        "rectangle_preserve" => (
            Options {
                preserve_vertices: true,
                ..base()
            },
            Box::new(|o, r| shape::rectangle(0.0, 0.0, 220.0, 128.0, o, r)),
        ),
        "rectangle_rough0" => (
            Options {
                roughness: 0.0,
                ..base()
            },
            Box::new(|o, r| shape::rectangle(0.0, 0.0, 220.0, 128.0, o, r)),
        ),
        "ellipse_plain" => (
            Options {
                curve_fitting: 1.0,
                ..base()
            },
            Box::new(|o, r| shape::ellipse(Point::new(50.0, 30.0), 100.0, 60.0, o, r)),
        ),
        "ellipse_solid" => (
            Options {
                curve_fitting: 1.0,
                ..filled(FillStyle::Solid)
            },
            Box::new(|o, r| shape::ellipse(Point::new(50.0, 30.0), 100.0, 60.0, o, r)),
        ),
        "ellipse_hachure" => (
            Options {
                curve_fitting: 1.0,
                ..filled(FillStyle::Hachure)
            },
            Box::new(|o, r| shape::ellipse(Point::new(50.0, 30.0), 100.0, 60.0, o, r)),
        ),
        "polygon_diamond" => (
            base(),
            Box::new(move |o, r| shape::polygon(&diamond, true, o, r)),
        ),
        "linear_path" => (
            base(),
            Box::new(move |o, r| shape::linear_path(&bent, o, r)),
        ),
        "curve" => (base(), Box::new(move |o, r| shape::curve(&bent, o, r))),
        "path_rounded_solid" => (
            Options {
                preserve_vertices: true,
                ..filled(FillStyle::Solid)
            },
            Box::new(move |o, r| shape::path(rounded, o, r)),
        ),
        "curve_solid" => (
            filled(FillStyle::Solid),
            Box::new(move |o, r| shape::curve(&bent, o, r)),
        ),
        "path_rounded" => (
            Options {
                preserve_vertices: true,
                ..base()
            },
            Box::new(move |o, r| shape::path(rounded, o, r)),
        ),
        "line" => (
            base(),
            Box::new(|o, r| {
                shape::polygon(
                    &[Point::new(0.0, 0.0), Point::new(176.0, 42.0)],
                    false,
                    o,
                    r,
                )
            }),
        ),
        other => panic!("{other} has no drawing here"),
    };
    let mut random = options.random();
    draw(&options, &mut random)
}

macro_rules! cases {
    ($($name:ident,)*) => {
        $(
            #[test]
            fn $name() {
                matches(stringify!($name), &drawn(stringify!($name)));
            }
        )*
    };
}

cases! {
    line,
    rectangle_plain,
    rectangle_preserve,
    rectangle_rough0,
    rectangle_solid,
    rectangle_hachure,
    rectangle_crosshatch,
    rectangle_zigzag,
    ellipse_plain,
    ellipse_solid,
    ellipse_hachure,
    polygon_diamond,
    linear_path,
    curve,
    path_rounded,
    path_rounded_solid,
    curve_solid,
}
