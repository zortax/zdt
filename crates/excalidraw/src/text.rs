//! Measuring and wrapping the words in a drawing.
//!
//! Nothing here can measure a letter: only the host knows which faces are installed and how wide
//! they draw. So the host implements [`Measure`] and this decides where the lines break, where the
//! words sit inside a shape, and how large a box they need.

use crate::element::{Element, FontFamily, Kind, TextAlign, VerticalAlign};

/// How far the words inside a shape are held off its edge.
pub const BOUND_TEXT_PADDING: f64 = 5.0;
/// How much of an arrow's length its label may take.
pub const ARROW_LABEL_WIDTH_FRACTION: f64 = 0.7;
/// How narrow an arrow's label may be, as a multiple of the font size.
pub const ARROW_LABEL_MIN_WIDTH_RATIO: f64 = 11.0;

/// One face at one size.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Font {
    /// Which face.
    pub family: FontFamily,
    /// How tall the letters are.
    pub size: f64,
}

/// What can measure words.
pub trait Measure {
    /// How wide `text` is drawn in `font`, as one line.
    fn line_width(&self, text: &str, font: Font) -> f64;
}

/// How tall `lines` lines are.
#[must_use]
pub fn height(lines: usize, font_size: f64, line_height: f64) -> f64 {
    font_size * line_height * lines.max(1) as f64
}

/// How wide the widest line of `text` is.
#[must_use]
pub fn width(measure: &dyn Measure, text: &str, font: Font) -> f64 {
    text.split('\n')
        .map(|line| measure.line_width(line, font))
        .fold(0.0_f64, f64::max)
}

/// `text` broken so that no line is wider than `limit`.
///
/// The breaks go at spaces where they can, and inside a word only when the word alone is too wide.
/// A line that already has a break in it keeps it.
#[must_use]
pub fn wrap(measure: &dyn Measure, text: &str, font: Font, limit: f64) -> String {
    if limit <= 0.0 {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    for (at, paragraph) in text.split('\n').enumerate() {
        if at > 0 {
            out.push('\n');
        }
        wrap_one(measure, paragraph, font, limit, &mut out);
    }
    out
}

/// One paragraph, wrapped into `out`.
fn wrap_one(measure: &dyn Measure, text: &str, font: Font, limit: f64, out: &mut String) {
    let mut line = String::new();
    for word in words(text) {
        let candidate = if line.is_empty() {
            word.to_owned()
        } else {
            format!("{line}{word}")
        };
        if measure.line_width(candidate.trim_end(), font) <= limit || line.is_empty() {
            line = candidate;
            continue;
        }
        out.push_str(line.trim_end());
        out.push('\n');
        line = word.trim_start().to_owned();
    }
    // A word wider than the whole line is broken wherever it has to be.
    while measure.line_width(&line, font) > limit && line.chars().count() > 1 {
        let mut kept = String::new();
        for letter in line.chars() {
            let mut candidate = kept.clone();
            candidate.push(letter);
            if measure.line_width(&candidate, font) > limit && !kept.is_empty() {
                break;
            }
            kept = candidate;
        }
        let rest = line[kept.len()..].to_owned();
        out.push_str(&kept);
        out.push('\n');
        line = rest;
    }
    out.push_str(&line);
}

/// `text` split so that each piece carries the space that follows it.
fn words(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut in_space = false;
    for (at, letter) in text.char_indices() {
        let space = letter == ' ' || letter == '\t';
        if space {
            in_space = true;
        } else if in_space {
            out.push(&text[start..at]);
            start = at;
            in_space = false;
        }
    }
    if start < text.len() {
        out.push(&text[start..]);
    }
    out
}

/// The box words bound to `container` are laid out in.
///
/// The answer is in the container's own space, so a turned container's label is placed and then
/// turned with it.
#[must_use]
pub fn container_box(container: &Element) -> kurbo::Rect {
    let (width, height) = (container.width, container.height);
    let (mut x, mut y) = (BOUND_TEXT_PADDING, BOUND_TEXT_PADDING);
    let (limit_width, limit_height) = match container.kind {
        // An ellipse's corners are outside it, so the words sit in the square inside the ellipse.
        Kind::Ellipse => {
            let inset = |side: f64| (side / 2.0) * (1.0 - std::f64::consts::FRAC_1_SQRT_2);
            x += inset(width);
            y += inset(height);
            (
                ((width / 2.0) * std::f64::consts::SQRT_2).round() - BOUND_TEXT_PADDING * 2.0,
                ((height / 2.0) * std::f64::consts::SQRT_2).round() - BOUND_TEXT_PADDING * 2.0,
            )
        }
        // A diamond's is the box between the middles of its four sides.
        Kind::Diamond => {
            x += width / 4.0;
            y += height / 4.0;
            (
                (width / 2.0).round() - BOUND_TEXT_PADDING * 2.0,
                (height / 2.0).round() - BOUND_TEXT_PADDING * 2.0,
            )
        }
        // An arrow's label sits along it, and never so narrow that a word cannot fit.
        Kind::Arrow => (
            (width * ARROW_LABEL_WIDTH_FRACTION).max(0.0),
            height.max(0.0),
        ),
        _ => (
            width - BOUND_TEXT_PADDING * 2.0,
            height - BOUND_TEXT_PADDING * 2.0,
        ),
    };
    kurbo::Rect::new(x, y, x + limit_width.max(0.0), y + limit_height.max(0.0))
}

/// How wide words bound to `container` may be drawn.
#[must_use]
pub fn container_width(container: &Element, font_size: f64) -> f64 {
    let box_ = container_box(container);
    if container.kind == Kind::Arrow {
        return box_.width().max(font_size * ARROW_LABEL_MIN_WIDTH_RATIO);
    }
    box_.width()
}

/// How large a container has to be to hold words `side` across, on that axis.
#[must_use]
pub fn container_size_for(side: f64, kind: Kind) -> f64 {
    match kind {
        Kind::Ellipse => {
            ((side + BOUND_TEXT_PADDING * 2.0) / std::f64::consts::SQRT_2 * 2.0).round()
        }
        Kind::Arrow => side + 80.0,
        Kind::Diamond => 2.0 * (side + BOUND_TEXT_PADDING * 2.0),
        _ => side + BOUND_TEXT_PADDING * 2.0,
    }
}

/// Where a run of words sits inside the box it was given.
#[must_use]
pub fn placed(
    box_: kurbo::Rect,
    text_width: f64,
    text_height: f64,
    across: TextAlign,
    down: VerticalAlign,
) -> kurbo::Point {
    let x = match across {
        TextAlign::Left => box_.x0,
        TextAlign::Center => box_.x0 + (box_.width() - text_width) / 2.0,
        TextAlign::Right => box_.x0 + (box_.width() - text_width),
    };
    let y = match down {
        VerticalAlign::Top => box_.y0,
        VerticalAlign::Middle => box_.y0 + (box_.height() - text_height) / 2.0,
        VerticalAlign::Bottom => box_.y0 + (box_.height() - text_height),
    };
    kurbo::Point::new(x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A face that draws every letter the same width, so a test can count letters.
    struct Monospace;

    impl Measure for Monospace {
        fn line_width(&self, text: &str, font: Font) -> f64 {
            text.chars().count() as f64 * font.size * 0.5
        }
    }

    fn font() -> Font {
        Font {
            family: FontFamily::Excalifont,
            size: 20.0,
        }
    }

    fn read(json: &str) -> Element {
        let value: serde_json::Value = serde_json::from_str(json).expect("valid JSON");
        crate::element::read(value.as_object().expect("an object")).expect("an element")
    }

    #[test]
    fn words_that_fit_are_left_on_one_line() {
        let out = wrap(&Monospace, "one two", font(), 200.0);
        assert_eq!(out, "one two");
    }

    #[test]
    fn a_line_too_wide_is_broken_at_a_space() {
        // Ten letters fit in a hundred at half of twenty.
        let out = wrap(&Monospace, "aaaa bbbb cccc", font(), 100.0);
        assert_eq!(out, "aaaa bbbb\ncccc");
    }

    #[test]
    fn a_word_wider_than_the_line_is_broken_inside_itself() {
        let out = wrap(&Monospace, "aaaaaaaaaaaaaa", font(), 60.0);
        assert_eq!(out, "aaaaaa\naaaaaa\naa");
    }

    #[test]
    fn a_break_that_was_typed_is_kept() {
        let out = wrap(&Monospace, "one\ntwo", font(), 200.0);
        assert_eq!(out, "one\ntwo");
    }

    #[test]
    fn the_height_is_the_lines_times_the_line_height() {
        assert!((height(3, 20.0, 1.25) - 75.0).abs() < f64::EPSILON);
        assert!((height(0, 20.0, 1.25) - 25.0).abs() < f64::EPSILON);
    }

    #[test]
    fn the_width_is_the_widest_line() {
        let out = width(&Monospace, "aa\naaaa", font());
        assert!((out - 40.0).abs() < f64::EPSILON);
    }

    #[test]
    fn a_rectangles_label_sits_inside_its_padding() {
        let held = read(r#"{"type":"rectangle","width":200,"height":100}"#);
        let box_ = container_box(&held);
        assert!((box_.x0 - 5.0).abs() < f64::EPSILON);
        assert!((box_.width() - 190.0).abs() < f64::EPSILON);
    }

    #[test]
    fn an_ellipses_label_sits_in_the_square_inside_it() {
        let held = read(r#"{"type":"ellipse","width":200,"height":200}"#);
        let box_ = container_box(&held);
        assert!(box_.x0 > 5.0, "it is held further off than a rectangle's");
        assert!(box_.width() < 200.0);
    }

    #[test]
    fn an_arrows_label_is_never_too_narrow_for_a_word() {
        let held = read(r#"{"type":"arrow","points":[[0,0],[40,0]]}"#);
        assert!((container_width(&held, 20.0) - 220.0).abs() < f64::EPSILON);
    }

    #[test]
    fn words_sit_where_they_are_aligned_to() {
        let box_ = kurbo::Rect::new(0.0, 0.0, 100.0, 100.0);
        let left = placed(box_, 40.0, 20.0, TextAlign::Left, VerticalAlign::Top);
        assert!((left.x).abs() < f64::EPSILON && (left.y).abs() < f64::EPSILON);
        let middle = placed(box_, 40.0, 20.0, TextAlign::Center, VerticalAlign::Middle);
        assert!((middle.x - 30.0).abs() < f64::EPSILON);
        assert!((middle.y - 40.0).abs() < f64::EPSILON);
        let end = placed(box_, 40.0, 20.0, TextAlign::Right, VerticalAlign::Bottom);
        assert!((end.x - 60.0).abs() < f64::EPSILON);
        assert!((end.y - 80.0).abs() < f64::EPSILON);
    }
}
