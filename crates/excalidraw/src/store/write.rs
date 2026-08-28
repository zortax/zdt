//! Writing the document back out the way JavaScript would have.
//!
//! Excalidraw writes `JSON.stringify(data, null, 2)`, so a file this crate writes has to indent by
//! two and separate a key from its value by one space. The other half is numbers: JavaScript has
//! one number type and prints a whole one without a point, so a coordinate of `21801` is written
//! `21801` and never `21801.0`. [`Number`] is what puts a number into the document in that form.

use serde_json::Value;
use serde_json::ser::PrettyFormatter;

/// A number, written the way JavaScript writes it.
///
/// A whole number inside the range JavaScript counts exactly in becomes an integer, and everything
/// else stays a decimal. That is what keeps a file this crate saves byte-identical to the one it
/// read where nothing changed.
pub struct Number;

/// The largest whole number JavaScript counts without losing one.
const SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

impl Number {
    /// `value` as a JSON number.
    #[must_use]
    pub fn json(value: f64) -> Value {
        if !value.is_finite() {
            // JavaScript writes these as `null`, and so does this.
            return Value::Null;
        }
        if value.fract() == 0.0 && value.abs() <= SAFE_INTEGER {
            // `-0` is written `0`, as JavaScript writes it.
            #[allow(clippy::cast_possible_truncation)]
            let whole = value as i64;
            return Value::from(whole);
        }
        serde_json::Number::from_f64(value).map_or(Value::Null, Value::Number)
    }
}

/// `document`, written the way Excalidraw writes it.
///
/// # Errors
///
/// If the document holds something that cannot be written as JSON, which a document built from
/// JSON never does.
pub fn to_string_pretty(document: &Value) -> Result<String, serde_json::Error> {
    let mut out = Vec::new();
    let formatter = PrettyFormatter::with_indent(b"  ");
    let mut serializer = serde_json::Serializer::with_formatter(&mut out, formatter);
    serde::Serialize::serialize(document, &mut serializer)?;
    Ok(String::from_utf8(out).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_whole_number_is_written_without_a_point() {
        assert_eq!(Number::json(21801.0).to_string(), "21801");
        assert_eq!(Number::json(0.0).to_string(), "0");
        assert_eq!(Number::json(-0.0).to_string(), "0");
        assert_eq!(Number::json(-42.0).to_string(), "-42");
    }

    #[test]
    fn a_fraction_keeps_its_point() {
        assert_eq!(Number::json(719.5).to_string(), "719.5");
        assert_eq!(Number::json(0.25).to_string(), "0.25");
    }

    #[test]
    fn a_number_that_is_not_one_is_written_as_nothing() {
        assert_eq!(Number::json(f64::NAN), Value::Null);
        assert_eq!(Number::json(f64::INFINITY), Value::Null);
    }

    #[test]
    fn the_document_is_indented_by_two() {
        let document = serde_json::json!({ "type": "excalidraw", "elements": [] });
        assert_eq!(
            to_string_pretty(&document).expect("it writes"),
            "{\n  \"type\": \"excalidraw\",\n  \"elements\": []\n}"
        );
    }
}
