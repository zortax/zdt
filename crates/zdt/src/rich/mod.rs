//! Rich presentations of buffers.
//!
//! A buffer holds text; a split chooses how to draw it. Most buffers have one drawn form, the
//! editor. A buffer whose content *is* a document — markdown today, images and tables later —
//! can also be drawn as that document. Which form a split shows is per `(window, buffer)`, so
//! one split can hold the source while another holds the page.
//!
//! [`RichKind`] names the forms that exist and answers which one a buffer has. The preview keeps
//! its reading position in a [`Reading`], filed with [`Previews`] the way editors are filed with
//! the workspace, so the keys can reach the one under the keyboard.

pub mod excalidraw;
pub mod image;
pub mod images;
mod pill;
pub mod stage;
pub mod svg;
pub mod toolbar;
mod view;

pub use pill::{ViewPill, ViewPillProps};
pub use view::{MarkdownPreview, MarkdownPreviewProps};

use std::cell::RefCell;
use std::rc::Rc;

use rustc_hash::FxHashMap;
use zgui::reactive::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};

use crate::workspace::{Buffer, BufferId, BufferKind, WindowId, Workspace};

/// The keymap overlay a focused preview answers keys in.
pub const REGION: &str = "preview";

/// Which rich presentation a buffer can have.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RichKind {
    /// Markdown, drawn as a document.
    Markdown,
    /// A raster picture, drawn by the image preview.
    Image,
    /// A vector drawing, drawn by the SVG preview and edited in place.
    Svg,
    /// A hand-drawn diagram, drawn by the Excalidraw editor.
    Excalidraw,
}

impl RichKind {
    /// The rich presentation of `buffer`, when it has one.
    #[must_use]
    pub fn of(buffer: &Buffer) -> Option<Self> {
        // By the name, before the language: a drawing is JSON, and a plain `.json` file has no
        // rich form.
        if buffer
            .path
            .as_deref()
            .is_some_and(|path| has_extension(path, "excalidraw"))
        {
            return Some(Self::Excalidraw);
        }
        match (&buffer.kind, buffer.language()) {
            (BufferKind::Text { .. }, Some("markdown")) => Some(Self::Markdown),
            (BufferKind::Text { .. }, Some("svg")) => Some(Self::Svg),
            (BufferKind::Image { .. }, _) => Some(Self::Image),
            _ => None,
        }
    }

    /// The rich presentation a file at `path` would open with, when it has one.
    ///
    /// For the open path: a kind with no text source is opened from the path alone, and the file
    /// is never read as text.
    #[must_use]
    pub fn for_path(path: &std::path::Path) -> Option<Self> {
        let extension = path.extension()?.to_str()?.to_ascii_lowercase();
        match extension.as_str() {
            "png" | "jpg" | "jpeg" | "gif" | "webp" => Some(Self::Image),
            _ => None,
        }
    }

    /// Whether the buffer also has a text source to show. An image has none.
    #[must_use]
    pub const fn has_source(self) -> bool {
        match self {
            Self::Markdown | Self::Svg | Self::Excalidraw => true,
            Self::Image => false,
        }
    }

    /// Which presentation a fresh view starts in.
    ///
    /// An SVG opens as the drawing: the drawing is what the file is for, and the source is one
    /// toggle away.
    #[must_use]
    pub const fn starts_in(self) -> Presentation {
        match self {
            Self::Markdown => Presentation::Source,
            Self::Image | Self::Svg | Self::Excalidraw => Presentation::Rich,
        }
    }
}

/// Which presentation a split shows a buffer in.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Presentation {
    /// The text, in an editor.
    #[default]
    Source,
    /// The rich form.
    Rich,
}

/// Whether `path` ends in `extension`, whatever case it was written in.
fn has_extension(path: &std::path::Path, extension: &str) -> bool {
    path.extension()
        .and_then(|held| held.to_str())
        .is_some_and(|held| held.eq_ignore_ascii_case(extension))
}

/// A density made safe to divide by: one of nothing would turn a length into infinity.
fn density(scale: f32) -> f32 {
    if scale.is_finite() && scale > 0.01 {
        scale
    } else {
        1.0
    }
}

/// `device` device pixels as CSS pixels, which is what the keys count in.
fn css(scale: f32, device: f32) -> f32 {
    device / density(scale)
}

/// `css` CSS pixels as device pixels, which is what a scroll container is moved in.
fn device(scale: f32, css: f32) -> f32 {
    css * density(scale)
}

/// How far one `j` moves the page, in pixels.
///
/// One line, so that walking down a preview moves it the same distance as walking down the file.
const LINE: f32 = 16.0;

/// One preview's reading position.
///
/// The offset is asked for and the view applies it, the same shape as the hover panel: the
/// engine owns the movement and re-fragments what it moves.
#[derive(Clone, Copy, PartialEq)]
pub struct Reading {
    /// How far down it has been scrolled, in pixels.
    offset: RwSignal<f32, LocalStorage>,
    /// How far it *can* be scrolled, which only a laid-out view knows.
    limit: RwSignal<f32, LocalStorage>,
    /// How tall the view is, for the keys that move by screenfuls.
    page: RwSignal<f32, LocalStorage>,
}

impl Reading {
    /// At the top, before the view has been measured.
    #[must_use]
    pub fn new() -> Self {
        Self {
            offset: RwSignal::new_local(0.0),
            limit: RwSignal::new_local(0.0),
            page: RwSignal::new_local(240.0),
        }
    }

    /// How far down it is. Tracked.
    #[must_use]
    pub fn offset(&self) -> f32 {
        self.offset.get()
    }

    /// Says how far it can be scrolled and how tall it is, from the measured view.
    pub fn set_extent(&self, limit: f32, page: f32) {
        if page > 1.0 {
            self.page.set(page);
        }
        let limit = limit.max(0.0);
        if (self.limit.get_untracked() - limit).abs() > 0.5 {
            self.limit.set(limit);
            // A view that shrank under a scrolled page would otherwise be scrolled past its end.
            let offset = self.offset.get_untracked();
            if offset > limit {
                self.offset.set(limit);
            }
        }
    }

    /// Moves the view by `lines`, clamped to what there is.
    pub fn scroll_lines(&self, lines: f32) {
        self.scroll_by(lines * LINE);
    }

    /// Moves it by `pixels`, clamped to what there is.
    pub fn scroll_by(&self, pixels: f32) {
        let limit = self.limit.get_untracked();
        let next = (self.offset.get_untracked() + pixels).clamp(0.0, limit);
        if (next - self.offset.get_untracked()).abs() > f32::EPSILON {
            self.offset.set(next);
        }
    }

    /// To the top, which `gg` does.
    pub fn to_top(&self) {
        self.offset.set(0.0);
    }

    /// To the bottom, which `G` does.
    pub fn to_bottom(&self) {
        self.offset.set(self.limit.get_untracked());
    }

    /// About a screen, for `<C-f>` and `<C-b>`. Half of it is `<C-d>` and `<C-u>`.
    #[must_use]
    pub fn page(&self) -> f32 {
        self.page.get_untracked()
    }
}

impl Default for Reading {
    fn default() -> Self {
        Self::new()
    }
}

/// Every mounted preview's reading position, by the window and buffer it belongs to.
///
/// No signal, the same reasoning as the workspace's editor handles: nothing on screen is decided
/// by which previews exist, and a key that scrolls one needs it right now.
#[derive(Clone)]
pub struct Previews {
    inner: Rc<RefCell<FxHashMap<(WindowId, BufferId), Reading>>>,
}

impl Previews {
    /// Nothing mounted.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Rc::new(RefCell::new(FxHashMap::default())),
        }
    }

    /// Remembers the preview showing `buffer` in `window`.
    pub fn register(&self, window: WindowId, buffer: BufferId, reading: Reading) {
        self.inner.borrow_mut().insert((window, buffer), reading);
    }

    /// Forgets it, which a view does as it unmounts.
    ///
    /// Dropped only when `reading` is still the one filed here, so a view rebuilt in place that
    /// registers its replacement first keeps the replacement.
    pub fn forget(&self, window: WindowId, buffer: BufferId, reading: Reading) {
        let mut held = self.inner.borrow_mut();
        if held.get(&(window, buffer)) == Some(&reading) {
            held.remove(&(window, buffer));
        }
    }

    /// The reading position of the preview the keyboard is in, when it is in one.
    #[must_use]
    pub fn current(&self, workspace: &Workspace) -> Option<Reading> {
        let window = workspace.focused_untracked();
        let buffer = workspace.buffer_in_untracked(window)?;
        if !workspace.is_rich_untracked(window, buffer) {
            return None;
        }
        self.inner.borrow().get(&(window, buffer)).copied()
    }
}

impl Default for Previews {
    fn default() -> Self {
        Self::new()
    }
}

/// Puts the registry where every component can find it.
pub fn provide(previews: Previews) {
    zgui::reactive::provide_local_context(previews);
}

/// It, from inside a component.
///
/// # Panics
///
/// If none was provided above this component, which is a wiring mistake.
#[must_use]
pub fn use_previews() -> Previews {
    zgui::reactive::use_local_context::<Previews>().expect("previews are provided at the root")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_files_have_a_rich_form() {
        let window = zgui_testkit_view::Window::open();
        window.scope.with(|| {
            let markdown = Buffer::text(
                BufferId::default(),
                Some("/tmp/readme.md".into()),
                zgui_editor::Document::new("# hi"),
            );
            assert_eq!(RichKind::of(&markdown), Some(RichKind::Markdown));

            let rust = Buffer::text(
                BufferId::default(),
                Some("/tmp/main.rs".into()),
                zgui_editor::Document::new("fn main() {}"),
            );
            assert_eq!(RichKind::of(&rust), None);
        });
    }

    #[test]
    fn pictures_and_drawings_have_their_own_kinds() {
        let window = zgui_testkit_view::Window::open();
        window.scope.with(|| {
            let picture = Buffer::image(BufferId::default(), "/tmp/photo.png".into());
            assert_eq!(RichKind::of(&picture), Some(RichKind::Image));
            assert!(!RichKind::Image.has_source());
            assert_eq!(RichKind::Image.starts_in(), Presentation::Rich);

            let drawing = Buffer::text(
                BufferId::default(),
                Some("/tmp/logo.svg".into()),
                zgui_editor::Document::new("<svg/>"),
            );
            assert_eq!(RichKind::of(&drawing), Some(RichKind::Svg));
            assert!(RichKind::Svg.has_source());
            assert_eq!(RichKind::Svg.starts_in(), Presentation::Rich);
        });
    }

    #[test]
    fn a_drawing_is_known_by_its_name_and_a_plain_json_file_is_not() {
        let window = zgui_testkit_view::Window::open();
        window.scope.with(|| {
            let drawing = Buffer::text(
                BufferId::default(),
                Some("/tmp/plan.excalidraw".into()),
                zgui_editor::Document::new("{}"),
            );
            assert_eq!(RichKind::of(&drawing), Some(RichKind::Excalidraw));
            assert!(RichKind::Excalidraw.has_source());
            assert_eq!(RichKind::Excalidraw.starts_in(), Presentation::Rich);

            let plain = Buffer::text(
                BufferId::default(),
                Some("/tmp/data.json".into()),
                zgui_editor::Document::new("{}"),
            );
            assert_eq!(RichKind::of(&plain), None);
        });
    }

    #[test]
    fn the_open_path_knows_a_picture_from_its_name() {
        use std::path::Path;
        assert_eq!(
            RichKind::for_path(Path::new("a/photo.PNG")),
            Some(RichKind::Image)
        );
        assert_eq!(
            RichKind::for_path(Path::new("a/photo.webp")),
            Some(RichKind::Image)
        );
        assert_eq!(RichKind::for_path(Path::new("a/logo.svg")), None);
        assert_eq!(RichKind::for_path(Path::new("a/main.rs")), None);
    }

    #[test]
    fn reading_clamps_to_the_measured_extent() {
        let window = zgui_testkit_view::Window::open();
        window.scope.with(|| {
            let reading = Reading::new();
            reading.set_extent(100.0, 240.0);
            reading.scroll_by(500.0);
            assert!((reading.offset.get_untracked() - 100.0).abs() < f32::EPSILON);
            // The view shrank: the offset follows the new end.
            reading.set_extent(40.0, 240.0);
            assert!((reading.offset.get_untracked() - 40.0).abs() < f32::EPSILON);
            reading.to_top();
            assert!(reading.offset.get_untracked().abs() < f32::EPSILON);
        });
    }
}
