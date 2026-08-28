//! The pending edits on one image view, and writing them into the file.
//!
//! Edits stack up in signals and touch no pixels until they are applied. The preview shows the
//! rotation and the flips as a transform, and the crop as an overlay, so stepping through tools
//! costs no decode. Applying re-encodes the file once, on a worker.

use zgui::reactive::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};

/// The pending edits: quarter turns, flips, and a crop.
///
/// The crop is fractions of the natural size, before the rotation: `[left, top, width, height]`
/// each in `0..=1`. Storing it unrotated is what lets [`applied`] cut first and turn after.
#[derive(Clone, Copy, PartialEq)]
pub struct Edits {
    /// Whether the edit tools are out.
    editing: RwSignal<bool, LocalStorage>,
    /// Whether the crop overlay is being dragged into place.
    cropping: RwSignal<bool, LocalStorage>,
    /// Clockwise quarter turns, `0..4`.
    quarter: RwSignal<u8, LocalStorage>,
    /// Mirrored left-to-right.
    flip_h: RwSignal<bool, LocalStorage>,
    /// Mirrored top-to-bottom.
    flip_v: RwSignal<bool, LocalStorage>,
    /// The kept part, when a crop is pending.
    crop: RwSignal<Option<[f32; 4]>, LocalStorage>,
}

/// The edits at one moment, for the worker that applies them.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Snapshot {
    pub quarter: u8,
    pub flip_h: bool,
    pub flip_v: bool,
    pub crop: Option<[f32; 4]>,
}

impl Edits {
    /// Nothing pending.
    #[must_use]
    pub fn new() -> Self {
        Self {
            editing: RwSignal::new_local(false),
            cropping: RwSignal::new_local(false),
            quarter: RwSignal::new_local(0),
            flip_h: RwSignal::new_local(false),
            flip_v: RwSignal::new_local(false),
            crop: RwSignal::new_local(None),
        }
    }

    /// Whether the edit tools are out. Tracked.
    #[must_use]
    pub fn editing(&self) -> bool {
        self.editing.get()
    }

    /// Puts the edit tools out or away. Leaving keeps the pending edits.
    pub fn set_editing(&self, on: bool) {
        self.editing.set(on);
        if !on {
            self.cropping.set(false);
        }
    }

    /// Whether the crop overlay is out. Tracked.
    #[must_use]
    pub fn cropping(&self) -> bool {
        self.cropping.get()
    }

    /// Puts the crop overlay out or away. Coming out starts from the whole picture.
    pub fn set_cropping(&self, on: bool) {
        if on && self.crop.get_untracked().is_none() {
            self.crop.set(Some([0.0, 0.0, 1.0, 1.0]));
        }
        self.cropping.set(on);
    }

    /// The pending crop. Tracked.
    #[must_use]
    pub fn crop(&self) -> Option<[f32; 4]> {
        self.crop.get()
    }

    /// Replaces the pending crop.
    pub fn set_crop(&self, rect: [f32; 4]) {
        self.crop.set(Some(rect));
    }

    /// Clockwise quarter turns. Tracked.
    #[must_use]
    pub fn quarter(&self) -> u8 {
        self.quarter.get()
    }

    /// Whether the picture is mirrored, on each axis. Tracked.
    #[must_use]
    pub fn flips(&self) -> (bool, bool) {
        (self.flip_h.get(), self.flip_v.get())
    }

    /// Turns the picture a quarter clockwise, or counter-clockwise for `-1`.
    pub fn rotate(&self, direction: i8) {
        let held = i16::from(self.quarter.get_untracked());
        let next = (held + i16::from(direction)).rem_euclid(4) as u8;
        self.quarter.set(next);
    }

    /// Mirrors the picture left-to-right.
    pub fn flip_horizontal(&self) {
        self.flip_h.set(!self.flip_h.get_untracked());
    }

    /// Mirrors it top-to-bottom.
    pub fn flip_vertical(&self) {
        self.flip_v.set(!self.flip_v.get_untracked());
    }

    /// Whether anything would change the file. Tracked.
    #[must_use]
    pub fn is_pending(&self) -> bool {
        self.quarter.get() != 0
            || self.flip_h.get()
            || self.flip_v.get()
            || self
                .crop
                .get()
                .is_some_and(|held| held != [0.0, 0.0, 1.0, 1.0])
    }

    /// Drops every pending edit.
    pub fn reset(&self) {
        self.quarter.set(0);
        self.flip_h.set(false);
        self.flip_v.set(false);
        self.crop.set(None);
        self.cropping.set(false);
    }

    /// The pending edits, for the worker. Nothing when the file would come back unchanged.
    #[must_use]
    pub fn snapshot(&self) -> Option<Snapshot> {
        let held = Snapshot {
            quarter: self.quarter.get_untracked(),
            flip_h: self.flip_h.get_untracked(),
            flip_v: self.flip_v.get_untracked(),
            crop: self
                .crop
                .get_untracked()
                .filter(|held| *held != [0.0, 0.0, 1.0, 1.0]),
        };
        (held != Snapshot::default()).then_some(held)
    }
}

impl Default for Edits {
    fn default() -> Self {
        Self::new()
    }
}

/// `image` with `edits` applied: the crop first, then the turns, then the mirrors.
#[must_use]
pub fn applied(image: image::DynamicImage, edits: &Snapshot) -> image::DynamicImage {
    let mut out = image;
    if let Some([x, y, w, h]) = edits.crop {
        let (width, height) = (out.width() as f32, out.height() as f32);
        let left = (x * width).round().clamp(0.0, width - 1.0) as u32;
        let top = (y * height).round().clamp(0.0, height - 1.0) as u32;
        let wide = ((w * width).round() as u32).clamp(1, out.width() - left);
        let tall = ((h * height).round() as u32).clamp(1, out.height() - top);
        out = out.crop_imm(left, top, wide, tall);
    }
    out = match edits.quarter % 4 {
        1 => out.rotate90(),
        2 => out.rotate180(),
        3 => out.rotate270(),
        _ => out,
    };
    if edits.flip_h {
        out = out.fliph();
    }
    if edits.flip_v {
        out = out.flipv();
    }
    out
}

/// Reads the file at `path`, applies `edits`, and writes it back atomically.
///
/// Answers the encoded bytes and the new pixel size, so the caller can show what was written
/// without reading the file again. Blocking. Called from a worker.
pub fn write(path: &std::path::Path, edits: &Snapshot) -> anyhow::Result<(Vec<u8>, (u32, u32))> {
    let format = image::ImageFormat::from_path(path)?;
    let out = applied(image::open(path)?, edits);
    // JPEG carries no alpha, and the encoder refuses texels that do.
    let out = if format == image::ImageFormat::Jpeg {
        image::DynamicImage::ImageRgb8(out.to_rgb8())
    } else {
        out
    };
    let size = (out.width(), out.height());

    let mut bytes = Vec::new();
    out.write_to(&mut std::io::Cursor::new(&mut bytes), format)?;
    zdt_core::fs::save_bytes(path, &bytes)?;
    Ok((bytes, size))
}

/// A crop rectangle carried from the stored, unturned space into the drawn one.
///
/// The overlay lives over the drawn picture, which has the turns and the mirrors already in it.
#[must_use]
pub fn to_display(rect: [f32; 4], quarter: u8, flip_h: bool, flip_v: bool) -> [f32; 4] {
    let [x, y, w, h] = rect;
    let turned = match quarter % 4 {
        1 => [1.0 - y - h, x, h, w],
        2 => [1.0 - x - w, 1.0 - y - h, w, h],
        3 => [y, 1.0 - x - w, h, w],
        _ => [x, y, w, h],
    };
    let [x, y, w, h] = turned;
    let x = if flip_h { 1.0 - x - w } else { x };
    let y = if flip_v { 1.0 - y - h } else { y };
    [x, y, w, h]
}

/// The inverse of [`to_display`]: a rectangle dragged on the drawn picture, stored unturned.
#[must_use]
pub fn from_display(rect: [f32; 4], quarter: u8, flip_h: bool, flip_v: bool) -> [f32; 4] {
    let [x, y, w, h] = rect;
    let x = if flip_h { 1.0 - x - w } else { x };
    let y = if flip_v { 1.0 - y - h } else { y };
    match quarter % 4 {
        1 => [y, 1.0 - x - w, h, w],
        2 => [1.0 - x - w, 1.0 - y - h, w, h],
        3 => [1.0 - y - h, x, h, w],
        _ => [x, y, w, h],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_crop_cuts_before_the_turn() {
        // A 4 by 2 picture with one red texel at the top left.
        let mut source = image::RgbaImage::new(4, 2);
        source.put_pixel(0, 0, image::Rgba([255, 0, 0, 255]));
        let edits = Snapshot {
            quarter: 1,
            crop: Some([0.0, 0.0, 0.5, 1.0]),
            ..Snapshot::default()
        };
        let out = applied(image::DynamicImage::ImageRgba8(source), &edits);
        // Half of 4 by 2 is 2 by 2; a quarter turn keeps it 2 by 2. The red texel turns from the
        // top left to the top right.
        assert_eq!((out.width(), out.height()), (2, 2));
        assert_eq!(
            out.to_rgba8().get_pixel(1, 0),
            &image::Rgba([255, 0, 0, 255])
        );
    }

    #[test]
    fn turns_swap_the_extent() {
        let source = image::DynamicImage::ImageRgba8(image::RgbaImage::new(4, 2));
        let turned = applied(
            source,
            &Snapshot {
                quarter: 3,
                ..Snapshot::default()
            },
        );
        assert_eq!((turned.width(), turned.height()), (2, 4));
    }

    #[test]
    fn the_display_mapping_round_trips() {
        let rect = [0.1, 0.2, 0.3, 0.4];
        for quarter in 0..4u8 {
            for flips in 0..4u8 {
                let (flip_h, flip_v) = (flips & 1 != 0, flips & 2 != 0);
                let there = to_display(rect, quarter, flip_h, flip_v);
                let back = from_display(there, quarter, flip_h, flip_v);
                for (one, two) in rect.iter().zip(back.iter()) {
                    assert!((one - two).abs() < 1e-6, "quarter {quarter} flips {flips}");
                }
            }
        }
    }

    #[test]
    fn a_quarter_turn_carries_the_top_left_to_the_top_right() {
        // The top-left tenth of the picture, turned a quarter clockwise, lands at the top right.
        let there = to_display([0.0, 0.0, 0.1, 0.1], 1, false, false);
        assert!((there[0] - 0.9).abs() < 1e-6);
        assert!(there[1].abs() < 1e-6);
    }

    #[test]
    fn nothing_pending_makes_no_snapshot() {
        let window = zgui_testkit_view::Window::open();
        window.scope.with(|| {
            let edits = Edits::new();
            assert!(edits.snapshot().is_none());
            // The whole picture is no crop.
            edits.set_cropping(true);
            assert!(edits.snapshot().is_none());
            edits.rotate(1);
            assert_eq!(edits.snapshot().expect("a turn is pending").quarter, 1);
            edits.rotate(-1);
            assert!(edits.snapshot().is_none());
        });
    }
}
