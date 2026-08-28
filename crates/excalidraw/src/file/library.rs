//! A library of drawings to stamp into one.
//!
//! A `.excalidrawlib` file holds a list of items, each a small group of elements. Version 1 held
//! the groups directly under `library`; version 2 wraps each in an item with a name and an id under
//! `libraryItems`. Both are read, because both are still passed around.

use serde_json::Value;

use crate::element::Element;

use super::Error;

/// What a library file calls itself.
pub const FILE_TYPE: &str = "excalidrawlib";
/// Which version of it this crate writes.
pub const VERSION: u64 = 2;

/// One thing in a library.
#[derive(Clone, PartialEq, Debug)]
pub struct Item {
    /// Which item this is.
    pub id: String,
    /// What it is called, when it is called anything.
    pub name: Option<String>,
    /// The elements it stamps, as the objects they were read from.
    pub elements: Vec<Value>,
    /// The same, as the rest of this crate reads them.
    pub parsed: Vec<Element>,
}

/// Everything a library file holds.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Library {
    /// The items, in the order the file lists them.
    pub items: Vec<Item>,
}

/// The library `text` holds.
///
/// # Errors
///
/// If the text is not JSON, or is JSON that does not call itself a library.
pub fn parse(text: &str) -> Result<Library, Error> {
    let document: Value = serde_json::from_str(text)?;
    let object = document.as_object().ok_or(Error::NotADrawing)?;
    if object.get("type").and_then(Value::as_str) != Some(FILE_TYPE) {
        return Err(Error::NotADrawing);
    }

    // Version 2 names its items; version 1 is a list of element lists with nothing around them.
    let items = if let Some(held) = object.get("libraryItems").and_then(Value::as_array) {
        held.iter().filter_map(item).collect()
    } else if let Some(held) = object.get("library").and_then(Value::as_array) {
        held.iter()
            .enumerate()
            .filter_map(|(at, held)| {
                Some(Item {
                    id: format!("item-{at}"),
                    name: None,
                    parsed: parsed(held.as_array()?),
                    elements: held.as_array()?.clone(),
                })
            })
            .collect()
    } else {
        return Err(Error::NotADrawing);
    };
    Ok(Library { items })
}

/// One named item.
fn item(value: &Value) -> Option<Item> {
    let object = value.as_object()?;
    let elements = object.get("elements")?.as_array()?.clone();
    Some(Item {
        id: crate::element::string(object, "id").unwrap_or_default(),
        name: crate::element::string(object, "name"),
        parsed: parsed(&elements),
        elements,
    })
}

/// The elements of one item, as the rest of this crate reads them.
fn parsed(elements: &[Value]) -> Vec<Element> {
    elements
        .iter()
        .filter_map(|held| crate::element::read(held.as_object()?))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_version_two_library_reads_its_items() {
        let library = parse(
            r#"{"type":"excalidrawlib","version":2,"libraryItems":[
                {"id":"a","name":"box","elements":[{"type":"rectangle","id":"r"}]}]}"#,
        )
        .expect("a library");
        assert_eq!(library.items.len(), 1);
        assert_eq!(library.items[0].name.as_deref(), Some("box"));
        assert_eq!(library.items[0].parsed.len(), 1);
    }

    #[test]
    fn a_version_one_library_still_reads() {
        let library = parse(
            r#"{"type":"excalidrawlib","version":1,"library":[
                [{"type":"rectangle","id":"r"}]]}"#,
        )
        .expect("a library");
        assert_eq!(library.items.len(), 1);
        assert!(library.items[0].name.is_none());
        assert_eq!(library.items[0].parsed.len(), 1);
    }

    #[test]
    fn a_drawing_is_not_a_library() {
        assert!(matches!(
            parse(r#"{"type":"excalidraw","elements":[]}"#),
            Err(Error::NotADrawing)
        ));
    }
}
