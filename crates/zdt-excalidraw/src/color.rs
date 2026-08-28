//! Turning the colours a drawing writes into ones the renderer paints with.
//!
//! Excalidraw writes hex, and `transparent` for nothing. A colour it cannot make sense of is drawn
//! in the ink colour rather than dropped, so a hand-edited file still shows its shapes.

use zgui::canvas::zgui_color::Color;

/// What a colour that cannot be read is drawn in.
const FALLBACK: Color = Color::srgb(0.118, 0.118, 0.118, 1.0);

/// The colour `text` names, at `alpha`.
///
/// `alpha` multiplies whatever the colour carries of its own, which is how an element's opacity
/// reaches the paint.
#[must_use]
pub fn of(text: &str, alpha: f64) -> Color {
    in_scheme(text, alpha, false)
}

/// The same, turned over for a dark surface when `dark` asks.
#[must_use]
pub fn in_scheme(text: &str, alpha: f64, dark: bool) -> Color {
    #[allow(clippy::cast_possible_truncation)]
    let alpha = alpha.clamp(0.0, 1.0) as f32;
    let held = parse(text).unwrap_or(FALLBACK);
    let held = if dark { darkened(held) } else { held };
    held.with_alpha(held.alpha() * alpha)
}

/// How much of a colour is turned over on a dark surface.
const INVERT: f64 = 0.93;

/// `colour`, as a dark surface shows it.
///
/// A drawing stores the colours it was drawn with, and a dark surface turns them over at the moment
/// they are painted — so the file is the same either way and a diagram written on white reads on
/// black. The two steps are an inversion and a half turn of the hue, which is what Excalidraw's own
/// `invert(93%) hue-rotate(180deg)` comes to.
#[must_use]
pub fn darkened(colour: Color) -> Color {
    let [r, g, b] = colour.components();
    let inverted = |value: f32| {
        let value = f64::from(value).clamp(0.0, 1.0);
        (value * (1.0 - INVERT) + (1.0 - value) * INVERT).clamp(0.0, 1.0)
    };
    let (r, g, b) = (inverted(r), inverted(g), inverted(b));

    // The hue-rotation matrix at half a turn, where the cosine is minus one and the sine is nothing.
    let rotated = |a: f64, b_: f64, c: f64| (r * a + g * b_ + b * c).clamp(0.0, 1.0);
    #[allow(clippy::cast_possible_truncation)]
    Color::srgb(
        rotated(0.213 - 0.787, 0.715 + 0.715, 0.072 + 0.072) as f32,
        rotated(0.213 + 0.213, 0.715 - 0.285, 0.072 + 0.072) as f32,
        rotated(0.213 + 0.213, 0.715 + 0.715, 0.072 - 0.928) as f32,
        colour.alpha(),
    )
}

/// `text`, as a dark surface shows it, written back as a colour a style sheet can take.
///
/// For the parts of a drawing that are drawn by the document rather than painted by the canvas:
/// its words, and the page behind them.
#[must_use]
pub fn css(text: &str, dark: bool) -> String {
    if !dark {
        return text.to_owned();
    }
    if is_nothing(text) {
        return "transparent".to_owned();
    }
    let held = darkened(parse(text).unwrap_or(FALLBACK));
    let [r, g, b] = held.components();
    let byte = |value: f32| {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let held = (value.clamp(0.0, 1.0) * 255.0).round() as u8;
        held
    };
    format!("#{:02x}{:02x}{:02x}", byte(r), byte(g), byte(b))
}

/// Whether `text` draws nothing at all.
#[must_use]
pub fn is_nothing(text: &str) -> bool {
    excalidraw::element::is_transparent(text)
}

/// The colour `text` names, when it names one.
#[must_use]
pub fn parse(text: &str) -> Option<Color> {
    let text = text.trim();
    if text.eq_ignore_ascii_case("transparent") {
        return Some(Color::srgb(0.0, 0.0, 0.0, 0.0));
    }
    if let Some(hex) = text.strip_prefix('#') {
        return hex_color(hex);
    }
    named(text)
}

/// The colour a run of hex digits names.
fn hex_color(hex: &str) -> Option<Color> {
    let digits: Vec<u8> = hex.bytes().map(hex_digit).collect::<Option<_>>()?;
    let (r, g, b, a) = match digits.len() {
        // The short forms double each digit, so `#f00` is `#ff0000`.
        3 => (digits[0] * 17, digits[1] * 17, digits[2] * 17, 255),
        4 => (
            digits[0] * 17,
            digits[1] * 17,
            digits[2] * 17,
            digits[3] * 17,
        ),
        6 => (
            digits[0] * 16 + digits[1],
            digits[2] * 16 + digits[3],
            digits[4] * 16 + digits[5],
            255,
        ),
        8 => (
            digits[0] * 16 + digits[1],
            digits[2] * 16 + digits[3],
            digits[4] * 16 + digits[5],
            digits[6] * 16 + digits[7],
        ),
        _ => return None,
    };
    Some(Color::srgb_u8(r, g, b, a))
}

/// The value one hex digit has.
fn hex_digit(letter: u8) -> Option<u8> {
    match letter {
        b'0'..=b'9' => Some(letter - b'0'),
        b'a'..=b'f' => Some(letter - b'a' + 10),
        b'A'..=b'F' => Some(letter - b'A' + 10),
        _ => None,
    }
}

/// The few colours a file names by word.
fn named(text: &str) -> Option<Color> {
    let (r, g, b) = match text.to_ascii_lowercase().as_str() {
        "black" => (0, 0, 0),
        "white" => (255, 255, 255),
        "red" => (255, 0, 0),
        "green" => (0, 128, 0),
        "blue" => (0, 0, 255),
        "yellow" => (255, 255, 0),
        "grey" | "gray" => (128, 128, 128),
        _ => return None,
    };
    Some(Color::srgb_u8(r, g, b, 255))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_six_digit_hex_reads() {
        let held = parse("#1e1e1e").expect("a colour");
        assert!((held.components()[0] - 30.0 / 255.0).abs() < 1e-6);
        assert!((held.alpha() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn the_short_form_doubles_each_digit() {
        assert_eq!(parse("#f00"), parse("#ff0000"));
        assert_eq!(parse("#f008"), parse("#ff000088"));
    }

    #[test]
    fn an_eight_digit_hex_carries_its_own_alpha() {
        let held = parse("#00000080").expect("a colour");
        assert!((held.alpha() - 128.0 / 255.0).abs() < 1e-6);
    }

    #[test]
    fn transparent_draws_nothing() {
        assert!(is_nothing("transparent"));
        assert!(is_nothing("TRANSPARENT"));
        assert!(!is_nothing("#ffffff"));
        assert!((parse("transparent").expect("a colour").alpha()).abs() < f64::EPSILON as f32);
    }

    #[test]
    fn opacity_multiplies_what_the_colour_carries() {
        let held = of("#ffffff", 0.4);
        assert!((held.alpha() - 0.4).abs() < 1e-6);
        let already = of("#ffffff80", 0.5);
        assert!((already.alpha() - 128.0 / 255.0 * 0.5).abs() < 1e-6);
    }

    #[test]
    fn a_colour_that_cannot_be_read_is_still_drawn() {
        let held = of("rgb(1 2 3)", 1.0);
        assert!(held.alpha() > 0.0, "it is drawn in the ink colour");
        assert!(parse("#12345").is_none());
        assert!(parse("nonsense").is_none());
    }

    #[test]
    fn a_dark_surface_turns_black_into_something_light() {
        let ink = of("#1e1e1e", 1.0);
        let turned = in_scheme("#1e1e1e", 1.0, true);
        let brightness = |held: Color| {
            let [r, g, b] = held.components();
            f64::from(r + g + b)
        };
        assert!(
            brightness(turned) > brightness(ink) + 1.5,
            "it is much lighter"
        );
        assert!((turned.alpha() - 1.0).abs() < 1e-6, "and just as solid");
    }

    #[test]
    fn a_dark_surface_turns_white_into_something_dark() {
        let paper = in_scheme("#ffffff", 1.0, true);
        let [r, g, b] = paper.components();
        assert!(f64::from(r + g + b) < 0.6, "the page went dark");
    }

    #[test]
    fn a_colour_keeps_roughly_its_hue_when_it_is_turned_over() {
        // Blue stays blue rather than becoming orange, which is what the half turn of the hue is
        // for: an inversion alone would swap every colour for its opposite.
        let blue = in_scheme("#1971c2", 1.0, true);
        let [r, _, b] = blue.components();
        assert!(b > r, "it is still more blue than red");
    }

    #[test]
    fn a_light_surface_leaves_every_colour_alone() {
        assert_eq!(of("#1e1e1e", 1.0), in_scheme("#1e1e1e", 1.0, false));
        assert_eq!(css("#1e1e1e", false), "#1e1e1e");
    }

    #[test]
    fn a_colour_a_style_sheet_takes_is_turned_over_too() {
        let turned = css("#1e1e1e", true);
        assert!(turned.starts_with('#') && turned.len() == 7, "{turned}");
        assert_ne!(turned, "#1e1e1e");
        assert_eq!(css("transparent", true), "transparent");
    }

    #[test]
    fn a_word_reads() {
        assert_eq!(parse("black"), parse("#000000"));
        assert_eq!(parse("WHITE"), parse("#ffffff"));
    }
}
