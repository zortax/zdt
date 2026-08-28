//! What perfect-freehand draws, drawn again here.
//!
//! `freehand.json` holds the outlines perfect-freehand 1.2.0 answers for the option set Excalidraw
//! passes, captured by `freehand.mjs`. The `freedraw` crate is the port under test; this checks it
//! is asked in the right way and answers the same points.
//!
//! Regenerate with:
//!
//! ```text
//! npm install perfect-freehand@1.2.0 && node tests/freehand.mjs > tests/freehand.json
//! ```

use excalidraw_rough::freehand::{DEFAULT_STREAMLINE, Stroke, Variability};
use kurbo::Point;
use serde_json::Value;

/// How far a point may be from the one JavaScript answered.
const TOLERANCE: f64 = 5e-6;

fn golden() -> Value {
    serde_json::from_str(include_str!("freehand.json")).expect("the capture is valid JSON")
}

/// The stroke every captured case was drawn from.
fn points() -> Vec<Point> {
    [
        (0.0, 0.0),
        (8.25, -4.5),
        (21.75, -12.25),
        (39.5, -19.75),
        (58.0, -24.0),
        (75.25, -23.5),
        (89.5, -17.25),
        (99.75, -6.5),
        (107.25, 8.5),
        (113.5, 24.25),
        (121.75, 33.75),
        (133.0, 36.25),
        (143.25, 33.0),
        (148.5, 27.5),
        (148.5, 27.5),
    ]
    .into_iter()
    .map(|(x, y)| Point::new(x, y))
    .collect()
}

const PRESSURES: [f64; 15] = [
    0.15, 0.32, 0.48, 0.61, 0.7, 0.74, 0.77, 0.79, 0.78, 0.72, 0.63, 0.51, 0.37, 0.21, 0.05,
];

fn matches(case: &str, drawn: &[Point]) {
    let golden = golden();
    let expected = golden
        .get(case)
        .unwrap_or_else(|| panic!("{case} is in the capture"))
        .as_array()
        .expect("a list of points");
    assert_eq!(
        drawn.len(),
        expected.len(),
        "{case}: {} points drawn, perfect-freehand drew {}",
        drawn.len(),
        expected.len()
    );
    for (at, (drawn, expected)) in drawn.iter().zip(expected).enumerate() {
        let expected = expected.as_array().expect("an x and a y");
        let (x, y) = (
            expected[0].as_f64().expect("a number"),
            expected[1].as_f64().expect("a number"),
        );
        assert!(
            (drawn.x - x).abs() < TOLERANCE && (drawn.y - y).abs() < TOLERANCE,
            "{case}: point {at} is {drawn:?}, perfect-freehand says ({x}, {y})"
        );
    }
}

#[test]
fn a_simulated_pressure_stroke_matches() {
    let points = points();
    let stroke = Stroke {
        points: &points,
        pressures: &[],
        simulate_pressure: true,
        stroke_width: 1.0,
        streamline: DEFAULT_STREAMLINE,
        variability: Variability::Variable,
    };
    matches("simulated", &stroke.outline());
}

#[test]
fn a_recorded_pressure_stroke_matches() {
    let points = points();
    let stroke = Stroke {
        points: &points,
        pressures: &PRESSURES,
        simulate_pressure: false,
        stroke_width: 2.0,
        streamline: DEFAULT_STREAMLINE,
        variability: Variability::Variable,
    };
    matches("recorded", &stroke.outline());
}

#[test]
fn a_single_tap_matches() {
    let points = [Point::ZERO];
    let stroke = Stroke {
        points: &points,
        pressures: &[0.5],
        simulate_pressure: false,
        stroke_width: 2.0,
        streamline: DEFAULT_STREAMLINE,
        variability: Variability::Variable,
    };
    matches("dot", &stroke.outline());
}

#[test]
fn a_two_point_stroke_matches() {
    let points = [Point::ZERO, Point::new(30.0, 10.0)];
    let stroke = Stroke {
        points: &points,
        pressures: &[0.5, 0.9],
        simulate_pressure: false,
        stroke_width: 2.0,
        streamline: DEFAULT_STREAMLINE,
        variability: Variability::Variable,
    };
    matches("two_points", &stroke.outline());
}
