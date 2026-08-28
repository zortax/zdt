//! The faces a drawing is written in.
//!
//! Excalidraw names a face by number and draws with faces of its own, so a drawing written
//! elsewhere reads correctly only if those faces are here. Three are shipped — the hand-drawn one a
//! drawing uses unless it says otherwise, the upright one, and the fixed-width one — and every
//! other number falls back to whatever the machine has.
//!
//! A host registers them once, before its first window.

use excalidraw::element::FontFamily;

/// The hand-drawn face, and the one a drawing uses unless it says otherwise.
pub const EXCALIFONT: &[u8] = include_bytes!("../assets/fonts/Excalifont-Regular.ttf");
/// The upright one.
pub const NUNITO: &[u8] = include_bytes!("../assets/fonts/Nunito-Regular.ttf");
/// The fixed-width one.
pub const COMIC_SHANNS: &[u8] = include_bytes!("../assets/fonts/ComicShanns-Regular.ttf");

/// Every face this crate ships, with the name a drawing asks for it by.
pub const SHIPPED: &[(&str, &[u8])] = &[
    ("Excalifont", EXCALIFONT),
    ("Nunito", NUNITO),
    ("Comic Shanns", COMIC_SHANNS),
];

/// Registers every shipped face with `fonts`.
///
/// A face that will not read is left out rather than failing the launch: a drawing that asks for it
/// falls back to whatever is installed, which is worse than the face and better than no window.
pub fn register(fonts: &zgui::app::Fonts) {
    for (name, bytes) in SHIPPED {
        let data: zgui::text::FontData = std::sync::Arc::new(*bytes);
        let _ = fonts.register(data, Some(name));
    }
}

/// Whether this crate ships the face `family` asks for.
///
/// A face it does not ship still draws, in whatever the machine has that reads like it.
#[must_use]
pub const fn is_shipped(family: FontFamily) -> bool {
    matches!(
        family,
        // The first three are the names their replacements took over.
        FontFamily::Virgil
            | FontFamily::Excalifont
            | FontFamily::Helvetica
            | FontFamily::Nunito
            | FontFamily::Cascadia
            | FontFamily::ComicShanns
    )
}

/// The face a drawing's number is actually drawn in.
///
/// The three renamed numbers answer their replacements, because those are the faces that are here.
#[must_use]
pub const fn drawn_as(family: FontFamily) -> FontFamily {
    match family {
        FontFamily::Virgil => FontFamily::Excalifont,
        FontFamily::Helvetica => FontFamily::Nunito,
        FontFamily::Cascadia => FontFamily::ComicShanns,
        held => held,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shipped_face_is_a_font_file() {
        for (name, bytes) in SHIPPED {
            assert!(bytes.len() > 10_000, "{name} is only {} bytes", bytes.len());
            // A TrueType file begins with the version it is.
            assert_eq!(&bytes[..4], b"\x00\x01\x00\x00", "{name} is not TrueType");
        }
    }

    #[test]
    fn a_renamed_face_is_drawn_as_the_one_that_replaced_it() {
        assert_eq!(drawn_as(FontFamily::Virgil), FontFamily::Excalifont);
        assert_eq!(drawn_as(FontFamily::Helvetica), FontFamily::Nunito);
        assert_eq!(drawn_as(FontFamily::Cascadia), FontFamily::ComicShanns);
        assert_eq!(drawn_as(FontFamily::LilitaOne), FontFamily::LilitaOne);
    }

    #[test]
    fn the_faces_that_are_here_are_the_ones_that_are_shipped() {
        assert!(is_shipped(FontFamily::Excalifont));
        assert!(is_shipped(FontFamily::Virgil));
        assert!(!is_shipped(FontFamily::LilitaOne));
    }
}
