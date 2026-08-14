//! Where a theme's text comes from.
//!
//! A theme is two blocks of custom-property declarations: one for a light surface and one for a
//! dark one. Two vocabularies share the block — `--zui-*`, which the component library's token
//! schema reads, and everything else, which this application reads straight from the cascade.
//! Both are carried together so that a person writing a theme writes one file per surface rather
//! than two.
//!
//! Built-in themes are compiled in. A theme in the configuration directory is read from disk and
//! reaches the interface through exactly the same type, so nothing downstream knows the
//! difference.

use std::path::Path;
use std::rc::Rc;

/// The declarations one theme lays over the base token set, for both surfaces.
#[derive(Clone, Debug)]
pub struct ThemeSource {
    /// How the theme is written in the configuration file.
    pub name: Rc<str>,
    /// The declarations in force on a light surface.
    pub light: Rc<str>,
    /// The declarations in force on a dark surface.
    pub dark: Rc<str>,
}

impl ThemeSource {
    /// A theme from two blocks of declarations.
    pub fn new(
        name: impl Into<Rc<str>>,
        light: impl Into<Rc<str>>,
        dark: impl Into<Rc<str>>,
    ) -> Self {
        Self {
            name: name.into(),
            light: light.into(),
            dark: dark.into(),
        }
    }
}

/// Declares the built-in themes and reads their two files.
macro_rules! builtins {
    ($($file:literal => $label:literal,)*) => {
        /// Every built-in theme, as it is written in the configuration file.
        pub const BUILTIN: &[(&str, &str)] = &[$(($file, $label)),*];

        /// The built-in theme written as `name`, if there is one.
        #[must_use]
        pub fn builtin_theme(name: &str) -> Option<ThemeSource> {
            match name {
                $(
                    $file => Some(ThemeSource::new(
                        $file,
                        include_str!(concat!("../../../assets/themes/", $file, "-light.css")),
                        include_str!(concat!("../../../assets/themes/", $file, "-dark.css")),
                    )),
                )*
                _ => None,
            }
        }
    };
}

builtins! {
    "oldworld" => "Oldworld",
}

/// The name and label of every built-in theme, in the order they are offered.
#[must_use]
pub fn builtin_theme_names() -> &'static [(&'static str, &'static str)] {
    BUILTIN
}

/// Reads a theme out of a directory, as `<name>-light.css` and `<name>-dark.css`.
///
/// A surface whose file is missing or unreadable falls back to the other one, so a person who
/// only cares about dark writes one file. A theme with neither file is not a theme, and this
/// answers `None` rather than an empty one that would silently render the interface unstyled.
#[must_use]
pub fn theme_from_dir(dir: &Path, name: &str) -> Option<ThemeSource> {
    let read =
        |suffix: &str| std::fs::read_to_string(dir.join(format!("{name}-{suffix}.css"))).ok();
    let light = read("light");
    let dark = read("dark");
    match (light, dark) {
        (None, None) => None,
        (light, dark) => {
            let light = light.or_else(|| dark.clone())?;
            let dark = dark.unwrap_or_else(|| light.clone());
            Some(ThemeSource::new(name, light, dark))
        }
    }
}

/// The theme written as `name`, preferring one in `dir` over a built-in of the same name.
#[must_use]
pub fn resolve_theme(dir: Option<&Path>, name: &str) -> Option<ThemeSource> {
    dir.and_then(|dir| theme_from_dir(dir, name))
        .or_else(|| builtin_theme(name))
}

#[cfg(test)]
mod tests {
    use super::{builtin_theme, resolve_theme};

    #[test]
    fn the_default_theme_carries_both_surfaces() {
        let theme = builtin_theme("oldworld").expect("oldworld is compiled in");
        assert!(theme.dark.contains("--editor-bg"));
        assert!(theme.light.contains("--editor-bg"));
        assert!(theme.dark.contains("--zui-color-background"));
    }

    #[test]
    fn an_unknown_name_is_not_a_theme() {
        assert!(builtin_theme("no-such-theme").is_none());
        assert!(resolve_theme(None, "no-such-theme").is_none());
    }

    #[test]
    fn every_syntax_capture_the_editor_knows_is_coloured() {
        // The editor falls back along the dots and then to the foreground, so a missing capture
        // renders as plain text rather than as an error. This is the only place that would say
        // so.
        let theme = builtin_theme("oldworld").expect("oldworld is compiled in");
        for capture in ["keyword", "string", "comment", "function", "type", "number"] {
            let property = format!("--syntax-{capture}:");
            assert!(theme.dark.contains(&property), "dark is missing {property}");
            assert!(
                theme.light.contains(&property),
                "light is missing {property}"
            );
        }
    }
}
