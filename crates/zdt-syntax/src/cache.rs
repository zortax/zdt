//! The shared cache: one parse per text, however many surfaces ask.
//!
//! Keys are content, so the same file asked for by two surfaces — or by the same surface after
//! a refresh that changed nothing — parses once. The store is bounded and forgets its oldest
//! answer first, which suits a diff: what is on screen was asked for last.

use std::collections::HashMap;
use std::collections::VecDeque;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex, OnceLock};

use zgui_editor::LanguageRegistry;

use crate::Highlights;
use crate::vocabulary::class_of;

/// How many texts the store keeps.
const KEEP: usize = 128;

/// One text's identity: its language, its length, and a hash of its bytes.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Key {
    language: &'static str,
    length: usize,
    hash: u64,
}

/// The languages and the answers, shared by every thread that highlights.
pub(crate) struct Service {
    registry: LanguageRegistry,
    held: Mutex<Store>,
}

/// The bounded map behind the lock.
#[derive(Default)]
struct Store {
    map: HashMap<Key, Arc<Highlights>>,
    order: VecDeque<Key>,
}

/// The one service.
pub(crate) fn shared() -> &'static Service {
    static SHARED: OnceLock<Service> = OnceLock::new();
    SHARED.get_or_init(|| Service {
        registry: LanguageRegistry::new().with_bundled(),
        held: Mutex::new(Store::default()),
    })
}

impl Service {
    /// The highlights of `text` as `language`, parsed at most once per content.
    pub(crate) fn of(&self, language: &'static str, text: &str) -> Option<Arc<Highlights>> {
        let config = self.registry.by_name(language)?;
        let key = Key {
            language,
            length: text.len(),
            hash: hash_of(text),
        };
        if let Some(found) = self.lock().map.get(&key) {
            return Some(Arc::clone(found));
        }

        // Parsing runs outside the lock, so a large file holds nobody else up.
        let answer = zgui_editor::highlight(&config, text)?;
        let classes = answer.captures.iter().map(|name| class_of(name)).collect();
        let made = Arc::new(Highlights {
            classes,
            lines: answer.lines,
        });

        let mut store = self.lock();
        if store.map.insert(key, Arc::clone(&made)).is_none() {
            store.order.push_back(key);
        }
        while store.order.len() > KEEP {
            if let Some(oldest) = store.order.pop_front() {
                store.map.remove(&oldest);
            }
        }
        Some(made)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Store> {
        self.held.lock().expect("the store lock is never poisoned")
    }
}

/// A content hash, joined with the length in the key so a collision needs both to agree.
fn hash_of(text: &str) -> u64 {
    let mut hasher = rustc_hash::FxHasher::default();
    text.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_text_is_parsed_once_and_shared() {
        let text = "fn main() {}\n";
        let Some(first) = shared().of("rust", text) else {
            // The grammar is a feature of the test build; without it there is nothing to test.
            return;
        };
        let second = shared().of("rust", text).expect("the answer is cached");
        assert!(Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn an_unknown_language_is_no_answer() {
        assert!(shared().of("no-such-language", "text").is_none());
    }
}
