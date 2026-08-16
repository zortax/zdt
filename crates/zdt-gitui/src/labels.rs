//! Turning what git says into what a person reads.

/// How long ago a unix timestamp was, roughly.
///
/// The same words the blame line uses, so that "3 days ago" means the same thing in both.
#[must_use]
pub fn ago(when: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs() as i64)
        .unwrap_or(when);
    let seconds = (now - when).max(0);

    let (count, unit) = match seconds {
        ..60 => return "just now".to_owned(),
        60..3_600 => (seconds / 60, "minute"),
        3_600..86_400 => (seconds / 3_600, "hour"),
        86_400..2_592_000 => (seconds / 86_400, "day"),
        2_592_000..31_536_000 => (seconds / 2_592_000, "month"),
        _ => (seconds / 31_536_000, "year"),
    };
    let plural = if count == 1 { "" } else { "s" };
    format!("{count} {unit}{plural} ago")
}

/// The same, short enough for a column: `3d`, `2mo`.
#[must_use]
pub fn ago_short(when: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs() as i64)
        .unwrap_or(when);
    let seconds = (now - when).max(0);

    match seconds {
        ..60 => "now".to_owned(),
        60..3_600 => format!("{}m", seconds / 60),
        3_600..86_400 => format!("{}h", seconds / 3_600),
        86_400..2_592_000 => format!("{}d", seconds / 86_400),
        2_592_000..31_536_000 => format!("{}mo", seconds / 2_592_000),
        _ => format!("{}y", seconds / 31_536_000),
    }
}

/// Everything a path could be, as one glyph and a tone.
#[must_use]
pub fn state_mark(state: zdt_git::State) -> (&'static str, &'static str) {
    use zdt_git::State;

    match state {
        State::Untracked => ("?", "zdt-git-untracked"),
        State::Added => ("A", "zdt-git-added"),
        State::Modified => ("M", "zdt-git-changed"),
        State::Deleted => ("D", "zdt-git-removed"),
        State::Renamed => ("R", "zdt-git-changed"),
        State::Conflicted => ("U", "zdt-git-conflict"),
        State::Unchanged => (" ", "zui-color-muted-foreground"),
    }
}

#[cfg(test)]
mod tests {
    use super::{ago, ago_short};

    /// A timestamp `seconds` ago.
    fn back(seconds: i64) -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("the clock is after 1970")
            .as_secs() as i64
            - seconds
    }

    #[test]
    fn a_moment_ago_is_just_now() {
        assert_eq!(ago(back(5)), "just now");
        assert_eq!(ago_short(back(5)), "now");
    }

    #[test]
    fn one_of_something_is_singular() {
        // "1 days ago" is the kind of thing that makes an interface feel unfinished.
        assert_eq!(ago(back(60 * 60 * 24)), "1 day ago");
        assert_eq!(ago(back(60 * 60 * 24 * 2)), "2 days ago");
        assert_eq!(ago(back(60 * 60)), "1 hour ago");
    }

    #[test]
    fn the_short_form_fits_a_column() {
        for seconds in [30, 300, 7_200, 86_400 * 3, 86_400 * 60, 86_400 * 800] {
            let short = ago_short(back(seconds));
            assert!(short.len() <= 4, "{short} is too wide for the column");
        }
    }

    #[test]
    fn a_timestamp_in_the_future_is_not_negative() {
        // Which happens with a clock that has been put back, and "-4 hours ago" is worse than
        // being a moment out.
        assert_eq!(ago(back(-500)), "just now");
    }

    #[test]
    fn every_unit_reads_in_the_largest_one_that_fits() {
        assert_eq!(ago(back(0)), "just now");
        assert_eq!(ago(back(120)), "2 minutes ago");
        assert_eq!(ago(back(3_600)), "1 hour ago");
        assert_eq!(ago(back(86_400 * 3)), "3 days ago");
        assert_eq!(ago(back(2_592_000 * 4)), "4 months ago");
        assert_eq!(ago(back(31_536_000 * 2)), "2 years ago");
    }
}
