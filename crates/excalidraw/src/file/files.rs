//! The pictures a drawing's image elements draw.
//!
//! A drawing carries its pictures with it, as data URLs under the id an image element names. The id
//! is the hash of the bytes, so the same picture placed twice is stored once.

use base64::Engine as _;
use rustc_hash::FxHashMap;
use serde_json::{Map, Value};

/// One picture, as the file holds it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BinaryFile {
    /// What kind of picture it is.
    pub mime_type: String,
    /// The bytes, still as the data URL they were written as.
    pub data_url: String,
    /// When it was added, as milliseconds since the epoch.
    pub created: u64,
    /// When it was last asked for.
    pub last_retrieved: Option<u64>,
}

impl BinaryFile {
    /// The picture's own bytes, decoded out of its data URL.
    ///
    /// Answers nothing for a URL that is not base64 data, which is the only form Excalidraw writes.
    #[must_use]
    pub fn bytes(&self) -> Option<Vec<u8>> {
        let (head, body) = self.data_url.split_once(',')?;
        if !head.starts_with("data:") || !head.ends_with(";base64") {
            return None;
        }
        base64::engine::general_purpose::STANDARD
            .decode(body.trim())
            .ok()
    }

    /// The picture `bytes` are, as a file to be stored.
    #[must_use]
    pub fn from_bytes(bytes: &[u8], mime_type: &str, created: u64) -> Self {
        let body = base64::engine::general_purpose::STANDARD.encode(bytes);
        Self {
            mime_type: mime_type.to_owned(),
            data_url: format!("data:{mime_type};base64,{body}"),
            created,
            last_retrieved: None,
        }
    }
}

/// Every picture a drawing holds, by the id its image elements name.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Files {
    inner: FxHashMap<String, BinaryFile>,
}

impl Files {
    /// The pictures `value` holds, when it holds any.
    #[must_use]
    pub fn read(value: Option<&Value>) -> Self {
        let Some(object) = value.and_then(Value::as_object) else {
            return Self::default();
        };
        let inner = object
            .iter()
            .filter_map(|(id, held)| {
                let held = held.as_object()?;
                Some((
                    id.clone(),
                    BinaryFile {
                        mime_type: crate::element::string(held, "mimeType")?,
                        data_url: crate::element::string(held, "dataURL")?,
                        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                        created: crate::element::number(held, "created").unwrap_or(0.0) as u64,
                        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                        last_retrieved: crate::element::number(held, "lastRetrieved")
                            .map(|held| held as u64),
                    },
                ))
            })
            .collect();
        Self { inner }
    }

    /// The picture `id` names.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&BinaryFile> {
        self.inner.get(id)
    }

    /// How many there are.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Whether there are none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Every id, in no order.
    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.inner.keys().map(String::as_str)
    }

    /// Files one picture under `id`.
    pub fn insert(&mut self, id: String, file: BinaryFile) {
        self.inner.insert(id, file);
    }

    /// One picture, as the object a file holds.
    #[must_use]
    pub fn entry_json(id: &str, file: &BinaryFile) -> Value {
        let mut object = Map::new();
        object.insert("mimeType".to_owned(), Value::String(file.mime_type.clone()));
        object.insert("id".to_owned(), Value::String(id.to_owned()));
        object.insert("dataURL".to_owned(), Value::String(file.data_url.clone()));
        object.insert("created".to_owned(), Value::from(file.created));
        if let Some(last) = file.last_retrieved {
            object.insert("lastRetrieved".to_owned(), Value::from(last));
        }
        Value::Object(object)
    }
}

/// The id `bytes` are filed under.
///
/// Excalidraw names a picture by the hex of its SHA-1, so the same picture placed twice is stored
/// once and two drawings that hold it agree on its name.
#[must_use]
pub fn id_of(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(40);
    for byte in sha1(bytes) {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// The SHA-1 of `bytes`.
fn sha1(bytes: &[u8]) -> [u8; 20] {
    let mut state: [u32; 5] = [
        0x6745_2301,
        0xefcd_ab89,
        0x98ba_dcfe,
        0x1032_5476,
        0xc3d2_e1f0,
    ];
    let mut message = bytes.to_vec();
    let length = (bytes.len() as u64) * 8;
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&length.to_be_bytes());

    for block in message.chunks_exact(64) {
        let mut words = [0u32; 80];
        for (at, chunk) in block.chunks_exact(4).enumerate() {
            words[at] = u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        }
        for at in 16..80 {
            words[at] =
                (words[at - 3] ^ words[at - 8] ^ words[at - 14] ^ words[at - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = state;
        for (at, word) in words.iter().enumerate() {
            let (mixed, constant) = match at {
                0..=19 => ((b & c) | ((!b) & d), 0x5a82_7999),
                20..=39 => (b ^ c ^ d, 0x6ed9_eba1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8f1b_bcdc),
                _ => (b ^ c ^ d, 0xca62_c1d6),
            };
            let next = a
                .rotate_left(5)
                .wrapping_add(mixed)
                .wrapping_add(e)
                .wrapping_add(constant)
                .wrapping_add(*word);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = next;
        }
        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
        state[4] = state[4].wrapping_add(e);
    }

    let mut out = [0u8; 20];
    for (at, word) in state.iter().enumerate() {
        out[at * 4..at * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_id_is_the_hash_the_web_app_uses() {
        // The two SHA-1 digests every implementation is checked against.
        assert_eq!(id_of(b""), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(id_of(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(
            id_of(b"The quick brown fox jumps over the lazy dog"),
            "2fd4e1c67a2d28fced849ee1bb76e7391b93eb12"
        );
    }

    #[test]
    fn a_picture_survives_being_written_and_read() {
        let bytes = [0u8, 1, 2, 250, 251, 252];
        let file = BinaryFile::from_bytes(&bytes, "image/png", 1_756_304_871_234);
        assert!(file.data_url.starts_with("data:image/png;base64,"));
        assert_eq!(file.bytes().expect("the bytes"), bytes);
    }

    #[test]
    fn a_url_that_is_not_base64_data_answers_nothing() {
        let file = BinaryFile {
            mime_type: "image/png".to_owned(),
            data_url: "https://example.invalid/cat.png".to_owned(),
            created: 0,
            last_retrieved: None,
        };
        assert!(file.bytes().is_none());
    }

    #[test]
    fn the_pictures_a_drawing_holds_are_read_by_their_ids() {
        let files = Files::read(Some(&serde_json::json!({
            "abc": { "mimeType": "image/png", "dataURL": "data:image/png;base64,AAA=", "created": 1 }
        })));
        assert_eq!(files.len(), 1);
        assert_eq!(
            files.get("abc").expect("the picture").mime_type,
            "image/png"
        );
    }
}
