//! The file a drawing is kept in.
//!
//! A `.excalidraw` file is one JSON object: what it is, which version of the format it is, where it
//! came from, the elements in painting order, the few settings that are saved with a drawing, and
//! the pictures its image elements draw.
//!
//! Nothing here reads the version. Excalidraw writes it and never looks at it — every difference
//! between one version of the format and the next is a missing field, and each of those is answered
//! where the field is read. So a file is recognised by its `type` alone, exactly as Excalidraw
//! recognises it.

pub mod files;
pub mod library;

use serde_json::{Map, Value};

use crate::element::Element;
use crate::store::{Number, Store};

pub use self::files::{BinaryFile, Files};

/// What a drawing file calls itself.
pub const FILE_TYPE: &str = "excalidraw";
/// Which version of the format this crate writes.
pub const VERSION: u64 = 2;
/// What this crate says wrote a file it makes.
pub const SOURCE: &str = "https://github.com/zortax/zdt";

/// Why a file could not be read.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// It is not JSON at all.
    #[error("not JSON: {0}")]
    Json(#[from] serde_json::Error),
    /// It is JSON, but not a drawing.
    #[error("not an Excalidraw drawing")]
    NotADrawing,
}

/// What is saved with a drawing, apart from the elements themselves.
#[derive(Clone, PartialEq, Debug)]
pub struct Settings {
    /// What the page behind the drawing is painted.
    pub background_color: String,
    /// How far apart the grid's lines are.
    pub grid_size: f64,
    /// How many of them there are between the heavy ones.
    pub grid_step: f64,
    /// Whether the grid is shown.
    pub grid_enabled: bool,
}

/// The page colour a drawing gets when its file does not say.
pub const DEFAULT_BACKGROUND: &str = "#ffffff";
/// How far apart the grid's lines are by default.
pub const DEFAULT_GRID_SIZE: f64 = 20.0;
/// How many lines are between the heavy ones.
pub const DEFAULT_GRID_STEP: f64 = 5.0;

impl Default for Settings {
    fn default() -> Self {
        Self {
            background_color: DEFAULT_BACKGROUND.to_owned(),
            grid_size: DEFAULT_GRID_SIZE,
            grid_step: DEFAULT_GRID_STEP,
            grid_enabled: false,
        }
    }
}

impl Settings {
    /// What `state` says, with a default for everything it does not.
    #[must_use]
    pub fn read(state: Option<&Map<String, Value>>) -> Self {
        let Some(state) = state else {
            return Self::default();
        };
        let default = Self::default();
        Self {
            background_color: crate::element::string(state, "viewBackgroundColor")
                .unwrap_or(default.background_color),
            grid_size: crate::element::number(state, "gridSize").unwrap_or(default.grid_size),
            grid_step: crate::element::number(state, "gridStep").unwrap_or(default.grid_step),
            grid_enabled: crate::element::flag(state, "gridModeEnabled")
                .unwrap_or(default.grid_enabled),
        }
    }

    /// These settings, as the object a file holds.
    #[must_use]
    pub fn to_json(&self) -> Value {
        let mut object = Map::new();
        object.insert("gridSize".to_owned(), Number::json(self.grid_size));
        object.insert("gridStep".to_owned(), Number::json(self.grid_step));
        object.insert("gridModeEnabled".to_owned(), Value::Bool(self.grid_enabled));
        object.insert(
            "viewBackgroundColor".to_owned(),
            Value::String(self.background_color.clone()),
        );
        object.insert(
            "lockedMultiSelections".to_owned(),
            Value::Object(Map::new()),
        );
        Value::Object(object)
    }
}

/// One drawing, read from a file.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Drawing {
    /// The JSON it was read from, which is what it is written back to.
    pub store: Store,
    /// The elements, in painting order, as the rest of this crate reads them.
    pub elements: Vec<Element>,
    /// The pictures its image elements draw.
    pub files: Files,
    /// What is saved with it apart from the elements.
    pub settings: Settings,
}

/// The drawing `text` holds.
///
/// # Errors
///
/// If the text is not JSON, or is JSON that does not call itself a drawing.
pub fn parse(text: &str) -> Result<Drawing, Error> {
    let document: Value = serde_json::from_str(text)?;
    from_value(document)
}

/// The drawing `document` holds.
///
/// # Errors
///
/// If it does not call itself a drawing.
pub fn from_value(document: Value) -> Result<Drawing, Error> {
    let object = document.as_object().ok_or(Error::NotADrawing)?;
    if object.get("type").and_then(Value::as_str) != Some(FILE_TYPE) {
        return Err(Error::NotADrawing);
    }
    if !object
        .get("elements")
        .is_none_or(|held| held.is_array() || held.is_null())
    {
        return Err(Error::NotADrawing);
    }

    let settings = Settings::read(object.get("appState").and_then(Value::as_object));
    let files = Files::read(object.get("files"));
    let store = Store::new(document);
    let elements = read_elements(&store);
    Ok(Drawing {
        store,
        elements,
        files,
        settings,
    })
}

/// Every element the store holds, in painting order.
///
/// A `selection` element, and anything the reader cannot make sense of, is left out — so the
/// elements and the store no longer line up index for index. [`Drawing::at`] is what maps between
/// them.
fn read_elements(store: &Store) -> Vec<Element> {
    store
        .elements()
        .iter()
        .filter_map(|held| crate::element::read(held.as_object()?))
        .collect()
}

impl Drawing {
    /// An empty drawing.
    #[must_use]
    pub fn new() -> Self {
        let mut object = Map::new();
        object.insert("type".to_owned(), Value::String(FILE_TYPE.to_owned()));
        object.insert("version".to_owned(), Value::from(VERSION));
        object.insert("source".to_owned(), Value::String(SOURCE.to_owned()));
        object.insert("elements".to_owned(), Value::Array(Vec::new()));
        let settings = Settings::default();
        object.insert("appState".to_owned(), settings.to_json());
        object.insert("files".to_owned(), Value::Object(Map::new()));
        Self {
            store: Store::new(Value::Object(object)),
            elements: Vec::new(),
            files: Files::default(),
            settings,
        }
    }

    /// Where in the store the element at `at` was read from.
    ///
    /// The two lists differ whenever a file holds something the reader dropped, so this walks the
    /// store looking for the id rather than assuming they line up.
    #[must_use]
    pub fn at(&self, at: usize) -> Option<usize> {
        let id = self.elements.get(at)?.id.as_str();
        self.store
            .elements()
            .iter()
            .position(|held| held.get("id").and_then(Value::as_str) == Some(id))
    }

    /// The element `id` names, and where it is in the list.
    #[must_use]
    pub fn find(&self, id: &crate::Id) -> Option<(usize, &Element)> {
        self.elements
            .iter()
            .enumerate()
            .find(|(_, held)| &held.id == id)
    }

    /// Reads every element out of the store again.
    ///
    /// What a command changed is written into the store, so this is how the view catches up.
    pub fn reread(&mut self) {
        self.elements = read_elements(&self.store);
        self.files = Files::read(self.store.document().get("files"));
        self.settings = Settings::read(
            self.store
                .document()
                .get("appState")
                .and_then(Value::as_object),
        );
    }

    /// The drawing as a file, written the way Excalidraw writes one.
    ///
    /// # Errors
    ///
    /// If the document holds something that cannot be written as JSON, which one read from JSON
    /// never does.
    pub fn to_string(&self) -> Result<String, serde_json::Error> {
        crate::store::to_string_pretty(self.store.document())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drawing_is_recognised_by_what_it_calls_itself() {
        assert!(parse(r#"{"type":"excalidraw","elements":[]}"#).is_ok());
        assert!(matches!(
            parse(r#"{"type":"excalidrawlib","libraryItems":[]}"#),
            Err(Error::NotADrawing)
        ));
        assert!(matches!(parse("not json"), Err(Error::Json(_))));
    }

    #[test]
    fn a_drawing_with_no_settings_takes_the_defaults() {
        let drawing = parse(r#"{"type":"excalidraw","elements":[]}"#).expect("a drawing");
        assert_eq!(drawing.settings, Settings::default());
    }

    #[test]
    fn the_settings_a_file_names_are_read() {
        let drawing = parse(
            r##"{"type":"excalidraw","elements":[],
                "appState":{"viewBackgroundColor":"#fffce8","gridSize":10,"gridModeEnabled":true}}"##,
        )
        .expect("a drawing");
        assert_eq!(drawing.settings.background_color, "#fffce8");
        assert!((drawing.settings.grid_size - 10.0).abs() < f64::EPSILON);
        assert!(drawing.settings.grid_enabled);
        // The one it did not name keeps its default.
        assert!((drawing.settings.grid_step - DEFAULT_GRID_STEP).abs() < f64::EPSILON);
    }

    #[test]
    fn a_dropped_element_does_not_move_the_ones_after_it() {
        let drawing = parse(
            r#"{"type":"excalidraw","elements":[
                {"type":"selection","id":"s"},
                {"type":"rectangle","id":"r"}]}"#,
        )
        .expect("a drawing");
        assert_eq!(drawing.elements.len(), 1);
        assert_eq!(drawing.elements[0].id.as_str(), "r");
        assert_eq!(drawing.at(0), Some(1), "it is the second in the store");
    }

    #[test]
    fn a_fresh_drawing_writes_the_shape_excalidraw_writes() {
        let written = Drawing::new().to_string().expect("it writes");
        assert!(written.starts_with("{\n  \"type\": \"excalidraw\",\n  \"version\": 2,"));
        let read = parse(&written).expect("what it wrote reads");
        assert!(read.elements.is_empty());
    }
}
