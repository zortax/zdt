//! Which face words are drawn in, and what that face measures.
//!
//! Excalidraw names a face by number, and the numbers have a history: the first three were renamed
//! when their replacements arrived, and both the old and the new numbers appear in files. The
//! metrics are the face's own, and the baseline is computed from them, so words land where they
//! were put by whichever editor wrote the file.

/// Which face words are drawn in.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum FontFamily {
    /// The original hand-drawn face. Files still name it; Excalifont replaced it.
    Virgil,
    /// The original upright face, replaced by Nunito.
    Helvetica,
    /// The original fixed-width face, replaced by Comic Shanns.
    Cascadia,
    /// The hand-drawn face, and the default.
    #[default]
    Excalifont,
    /// The upright one.
    Nunito,
    /// The heavy one.
    LilitaOne,
    /// The fixed-width one.
    ComicShanns,
    /// The one a document that asks for plain sans gets.
    LiberationSans,
    /// The other upright one.
    Assistant,
    /// The fallback for Chinese, Japanese and Korean.
    Xiaolai,
}

impl FontFamily {
    /// The face `number` names.
    ///
    /// An unknown number is the default, so a drawing made with a face this does not know still
    /// reads.
    #[must_use]
    pub const fn from_number(number: u32) -> Self {
        match number {
            1 => Self::Virgil,
            2 => Self::Helvetica,
            3 => Self::Cascadia,
            6 => Self::Nunito,
            7 => Self::LilitaOne,
            8 => Self::ComicShanns,
            9 => Self::LiberationSans,
            10 => Self::Assistant,
            100 => Self::Xiaolai,
            _ => Self::Excalifont,
        }
    }

    /// The number the file holds for this face.
    #[must_use]
    pub const fn to_number(self) -> u32 {
        match self {
            Self::Virgil => 1,
            Self::Helvetica => 2,
            Self::Cascadia => 3,
            Self::Excalifont => 5,
            Self::Nunito => 6,
            Self::LilitaOne => 7,
            Self::ComicShanns => 8,
            Self::LiberationSans => 9,
            Self::Assistant => 10,
            Self::Xiaolai => 100,
        }
    }

    /// How far apart its lines are, as a multiple of the font size.
    #[must_use]
    pub const fn line_height(self) -> f64 {
        match self {
            Self::Helvetica | Self::LilitaOne | Self::LiberationSans => 1.15,
            Self::Cascadia => 1.2,
            _ => 1.25,
        }
    }

    /// How many units of the face's own grid make one em.
    #[must_use]
    pub const fn units_per_em(self) -> f64 {
        match self {
            Self::Helvetica | Self::Cascadia | Self::LiberationSans | Self::Assistant => 2048.0,
            _ => 1000.0,
        }
    }

    /// How far above the baseline its tallest letters reach, in those units.
    #[must_use]
    pub const fn ascender(self) -> f64 {
        match self {
            Self::Virgil | Self::Excalifont => 886.0,
            Self::Helvetica => 1577.0,
            Self::Cascadia => 1900.0,
            Self::Nunito => 1011.0,
            Self::LilitaOne => 923.0,
            Self::ComicShanns => 750.0,
            Self::LiberationSans => 1854.0,
            Self::Assistant => 1021.0,
            Self::Xiaolai => 880.0,
        }
    }

    /// How far below it the lowest reach, as a negative number.
    #[must_use]
    pub const fn descender(self) -> f64 {
        match self {
            Self::Virgil | Self::Excalifont => -374.0,
            Self::Helvetica => -471.0,
            Self::Cascadia => -480.0,
            Self::Nunito => -353.0,
            Self::LilitaOne => -220.0,
            Self::ComicShanns => -250.0,
            Self::LiberationSans => -434.0,
            Self::Assistant => -287.0,
            Self::Xiaolai => -144.0,
        }
    }

    /// Whether it is a fixed-width face.
    #[must_use]
    pub const fn is_monospace(self) -> bool {
        matches!(self, Self::Cascadia | Self::ComicShanns)
    }

    /// What the face is called, for a host that has to find it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Virgil => "Virgil",
            Self::Helvetica => "Helvetica",
            Self::Cascadia => "Cascadia",
            Self::Excalifont => "Excalifont",
            Self::Nunito => "Nunito",
            Self::LilitaOne => "Lilita One",
            Self::ComicShanns => "Comic Shanns",
            Self::LiberationSans => "Liberation Sans",
            Self::Assistant => "Assistant",
            Self::Xiaolai => "Xiaolai",
        }
    }

    /// How far down a line box its baseline sits, at `font_size` and `line_height`.
    #[must_use]
    pub fn baseline(self, font_size: f64, line_height: f64) -> f64 {
        let em = font_size / self.units_per_em();
        let line = font_size * line_height;
        let gap = (line - em * self.ascender() + em * self.descender()) / 2.0;
        em * self.ascender() + gap
    }
}

/// How tall the letters are when a file does not say.
pub const DEFAULT_FONT_SIZE: f64 = 20.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_renamed_faces_keep_their_old_numbers() {
        assert_eq!(FontFamily::from_number(1), FontFamily::Virgil);
        assert_eq!(FontFamily::from_number(5), FontFamily::Excalifont);
        assert_eq!(FontFamily::from_number(4), FontFamily::Excalifont);
        assert_eq!(FontFamily::from_number(999), FontFamily::Excalifont);
    }

    #[test]
    fn every_face_writes_the_number_it_was_read_from() {
        for number in [1, 2, 3, 5, 6, 7, 8, 9, 10, 100] {
            assert_eq!(FontFamily::from_number(number).to_number(), number);
        }
    }

    #[test]
    fn the_baseline_sits_inside_the_line_box() {
        let face = FontFamily::Excalifont;
        let baseline = face.baseline(20.0, face.line_height());
        assert!(baseline > 0.0 && baseline < 20.0 * face.line_height());
    }
}
