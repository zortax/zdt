//! Formatting values back into attribute text.

use zgui::elements::kurbo;

/// A number as attribute text: three decimals, with the trailing zeros dropped.
#[must_use]
pub fn fmt(value: f64) -> String {
    let mut out = format!("{value:.3}");
    if out.contains('.') {
        while out.ends_with('0') {
            out.pop();
        }
        if out.ends_with('.') {
            out.pop();
        }
    }
    if out == "-0" { "0".to_owned() } else { out }
}

/// An affine as a `transform` attribute value. Identity is empty.
#[must_use]
pub fn transform_attr(affine: kurbo::Affine) -> String {
    if affine == kurbo::Affine::IDENTITY {
        return String::new();
    }
    let [a, b, c, d, e, f] = affine.as_coeffs();
    if a == 1.0 && b == 0.0 && c == 0.0 && d == 1.0 {
        return format!("translate({} {})", fmt(e), fmt(f));
    }
    format!(
        "matrix({} {} {} {} {} {})",
        fmt(a),
        fmt(b),
        fmt(c),
        fmt(d),
        fmt(e),
        fmt(f)
    )
}

/// Points as a `points` attribute value.
#[must_use]
pub fn points_attr(points: &[kurbo::Point]) -> String {
    points
        .iter()
        .map(|point| format!("{},{}", fmt(point.x), fmt(point.y)))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Text made safe inside a double-quoted attribute value.
#[must_use]
pub fn escaped(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_lose_their_trailing_zeros() {
        assert_eq!(fmt(10.0), "10");
        assert_eq!(fmt(10.5), "10.5");
        assert_eq!(fmt(10.125), "10.125");
        assert_eq!(fmt(1.0 / 3.0), "0.333");
        assert_eq!(fmt(-0.0001), "0");
    }

    #[test]
    fn a_plain_move_stays_readable() {
        assert_eq!(
            transform_attr(kurbo::Affine::translate((3.0, -4.5))),
            "translate(3 -4.5)"
        );
        assert_eq!(transform_attr(kurbo::Affine::IDENTITY), "");
        assert_eq!(
            transform_attr(kurbo::Affine::scale(2.0)),
            "matrix(2 0 0 2 0 0)"
        );
    }
}
