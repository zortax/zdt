//! What a copy puts on the clipboard.
//!
//! Excalidraw writes the same JSON to its own clipboard type and to plain text, so a copy crosses
//! between the web app and anything else that reads this. Plain text is what most desktops carry,
//! and it is what this writes and reads.

use serde_json::{Map, Value};

use crate::file::{Error, Files};
use crate::store::to_string_pretty;

/// What a clipboard payload calls itself.
pub const PAYLOAD_TYPE: &str = "excalidraw/clipboard";
/// What one made by a program calls itself.
pub const API_PAYLOAD_TYPE: &str = "excalidraw-api/clipboard";

/// What a copy carries.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Payload {
    /// The elements, as the objects they were read from.
    pub elements: Vec<Value>,
    /// The pictures the image elements among them draw.
    pub files: Files,
}

/// The payload `text` holds.
///
/// A whole drawing is accepted too, because dragging a `.excalidraw` file's contents onto a canvas
/// is a paste as far as the reader is concerned.
///
/// # Errors
///
/// If the text is not JSON, or is JSON that is neither a payload nor a drawing.
pub fn parse(text: &str) -> Result<Payload, Error> {
    let document: Value = serde_json::from_str(text)?;
    let object = document.as_object().ok_or(Error::NotADrawing)?;
    let kind = object.get("type").and_then(Value::as_str);
    if !matches!(
        kind,
        Some(PAYLOAD_TYPE | API_PAYLOAD_TYPE | crate::file::FILE_TYPE)
    ) {
        return Err(Error::NotADrawing);
    }
    let elements = object
        .get("elements")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    Ok(Payload {
        elements,
        files: Files::read(object.get("files")),
    })
}

impl Payload {
    /// The payload, as the text a copy puts on the clipboard.
    ///
    /// # Errors
    ///
    /// If it holds something that cannot be written as JSON, which one read from JSON never does.
    pub fn to_string(&self) -> Result<String, serde_json::Error> {
        let mut object = Map::new();
        object.insert("type".to_owned(), Value::String(PAYLOAD_TYPE.to_owned()));
        object.insert("elements".to_owned(), Value::Array(self.elements.clone()));
        let mut files = Map::new();
        for id in self.files.ids() {
            if let Some(file) = self.files.get(id) {
                files.insert(id.to_owned(), Files::entry_json(id, file));
            }
        }
        object.insert("files".to_owned(), Value::Object(files));
        to_string_pretty(&Value::Object(object))
    }

    /// The elements this crate reads out of the payload.
    #[must_use]
    pub fn parsed(&self) -> Vec<crate::Element> {
        self.elements
            .iter()
            .filter_map(|held| crate::element::read(held.as_object()?))
            .collect()
    }

    /// Whether it carries nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.elements.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_payload_survives_being_written_and_read() {
        let payload = Payload {
            elements: vec![serde_json::json!({ "type": "rectangle", "id": "a" })],
            files: Files::default(),
        };
        let text = payload.to_string().expect("it writes");
        let read = parse(&text).expect("it reads");
        assert_eq!(read.elements.len(), 1);
        assert_eq!(read.parsed()[0].kind, crate::Kind::Rectangle);
    }

    #[test]
    fn what_the_web_app_writes_reads() {
        let read = parse(
            r#"{"type":"excalidraw/clipboard","elements":[{"type":"ellipse","id":"e"}],"files":{}}"#,
        )
        .expect("it reads");
        assert_eq!(read.parsed()[0].kind, crate::Kind::Ellipse);
    }

    #[test]
    fn a_whole_drawing_pastes_too() {
        let read = parse(r#"{"type":"excalidraw","elements":[{"type":"diamond","id":"d"}]}"#)
            .expect("it reads");
        assert_eq!(read.parsed()[0].kind, crate::Kind::Diamond);
    }

    #[test]
    fn anything_else_is_not_a_paste() {
        assert!(parse("hello").is_err());
        assert!(parse(r#"{"type":"excalidrawlib","library":[]}"#).is_err());
    }

    #[test]
    fn a_payload_carries_the_pictures_its_images_draw() {
        let mut files = Files::default();
        files.insert(
            "abc".to_owned(),
            crate::file::BinaryFile::from_bytes(b"png", "image/png", 1),
        );
        let payload = Payload {
            elements: vec![serde_json::json!({ "type": "image", "id": "i", "fileId": "abc" })],
            files,
        };
        let text = payload.to_string().expect("it writes");
        let read = parse(&text).expect("it reads");
        assert_eq!(read.files.len(), 1);
        assert_eq!(
            read.files
                .get("abc")
                .expect("the picture")
                .bytes()
                .expect("bytes"),
            b"png"
        );
    }
}
