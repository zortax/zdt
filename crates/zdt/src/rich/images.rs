//! Fetching the pictures a document points at over the network.
//!
//! A document's remote images land in a cache under the state directory, named by a hash of
//! their address, and the same address is fetched once for the life of the process. The views
//! ask through [`zdt_view::markdown::Remote`] and draw whatever the signal answers: nothing
//! while the bytes are on their way or the fetch failed, and the file once they have landed.

use std::cell::RefCell;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::rc::Rc;

use rustc_hash::FxHashMap;
use zgui::reactive::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};
use zgui::task::blocking;

/// Every remote image this process has been asked for.
#[derive(Clone)]
pub struct Images {
    inner: Rc<Inner>,
}

struct Inner {
    /// Where fetched images live across runs.
    cache: Option<PathBuf>,
    /// One signal per address, made once and answered forever.
    entries: RefCell<FxHashMap<String, RwSignal<Option<PathBuf>, LocalStorage>>>,
    /// The owner the signals are created under. An image is asked for from a preview's scope
    /// and shown again from another after the first has gone.
    owner: zgui::reactive::Owner,
}

impl Images {
    /// An empty registry over the state directory's image cache.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Rc::new(Inner {
                cache: zdt_core::state::directory().map(|root| root.join("images")),
                entries: RefCell::new(FxHashMap::default()),
                owner: zgui::reactive::Owner::current().unwrap_or_default(),
            }),
        }
    }

    /// The file `url`'s bytes live in, as a signal that answers when they have landed.
    ///
    /// The first ask starts the fetch; every later ask for the same address shares its answer.
    pub fn fetch(&self, url: &str) -> RwSignal<Option<PathBuf>, LocalStorage> {
        if let Some(held) = self.inner.entries.borrow().get(url) {
            return *held;
        }

        let Some(cache) = self.inner.cache.clone() else {
            // Nowhere to put a file. The signal stays empty and the words stand in.
            let empty = self.inner.owner.with(|| RwSignal::new_local(None));
            self.inner
                .entries
                .borrow_mut()
                .insert(url.to_owned(), empty);
            return empty;
        };

        let file = cache.join(name_of(url));
        let signal = self
            .inner
            .owner
            .with(|| RwSignal::new_local(file.exists().then(|| file.clone())));
        self.inner
            .entries
            .borrow_mut()
            .insert(url.to_owned(), signal);
        if signal.get_untracked().is_some() {
            return signal;
        }

        // Detached, because the preview that asked is often toggled away before the bytes land,
        // and the cache is worth filling either way.
        let url = url.to_owned();
        zdt_view::detached(async move {
            let landed = blocking(move || download(&url, &cache, &file)).await;
            if let Some(file) = landed {
                signal.set(Some(file));
            }
        });
        signal
    }
}

impl Default for Images {
    fn default() -> Self {
        Self::new()
    }
}

/// Reads `url` into `file` under `cache`, and answers the file when everything held.
///
/// A failure answers nothing: the words keep standing in, and the next run asks again.
fn download(url: &str, cache: &std::path::Path, file: &std::path::Path) -> Option<PathBuf> {
    std::fs::create_dir_all(cache).ok()?;
    let mut response = ureq::get(url).call().ok()?;
    let bytes = response.body_mut().read_to_vec().ok()?;
    if bytes.is_empty() {
        return None;
    }
    // Written beside and moved into place, so a fetch that dies mid-write leaves no half image
    // for the next run to trust.
    let partial = file.with_extension("part");
    std::fs::write(&partial, &bytes).ok()?;
    std::fs::rename(&partial, file).ok()?;
    Some(file.to_path_buf())
}

/// The cache file an address lands in.
fn name_of(url: &str) -> String {
    let mut hasher = rustc_hash::FxHasher::default();
    url.hash(&mut hasher);
    let low = hasher.finish();
    let mut hasher = rustc_hash::FxHasher::default();
    (url, url.len()).hash(&mut hasher);
    format!("{low:016x}{:016x}.img", hasher.finish())
}
