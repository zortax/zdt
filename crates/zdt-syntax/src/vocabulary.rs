//! The capture names a theme colours, and the classes that carry them.
//!
//! The list is the editor's own `--syntax-*` vocabulary, written with dots as hyphens. Every
//! built-in theme declares each property, so a class needs no fallback chain of its own: a
//! capture outside the list falls back along its dots before a class is chosen.

/// Declares the vocabulary: each name, its class, and the rule that colours the class.
macro_rules! vocabulary {
    ($($name:literal,)*) => {
        /// Every colourable capture name, with the class that carries it.
        pub const NAMES: &[(&str, &str)] = &[$(($name, concat!("syn--", $name))),*];

        /// One rule per capture, over the `--syntax-*` custom properties the themes declare.
        pub const STYLE: &str = concat!($(
            ".syn--", $name, " { color: var(--syntax-", $name, "); }\n",
        )*);
    };
}

vocabulary! {
    "attribute",
    "comment",
    "comment-doc",
    "constant",
    "constant-builtin",
    "constructor",
    "embedded",
    "escape",
    "function",
    "function-builtin",
    "function-macro",
    "function-method",
    "keyword",
    "label",
    "module",
    "number",
    "operator",
    "property",
    "punctuation",
    "punctuation-bracket",
    "punctuation-delimiter",
    "punctuation-special",
    "string",
    "string-special",
    "tag",
    "type",
    "type-builtin",
    "variable",
    "variable-builtin",
    "variable-parameter",
}

/// The class for a capture like `function.method`: the exact name, then along its dots, then
/// nothing — which leaves the span the text colour around it.
#[must_use]
pub fn class_of(capture: &str) -> Option<&'static str> {
    let mut name = capture.replace('.', "-");
    loop {
        if let Some((_, class)) = NAMES.iter().find(|(held, _)| *held == name) {
            return Some(class);
        }
        match name.rfind('-') {
            Some(cut) => name.truncate(cut),
            None => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_fall_back_along_their_dots() {
        assert_eq!(class_of("function.method"), Some("syn--function-method"));
        assert_eq!(
            class_of("function.method.rare"),
            Some("syn--function-method")
        );
        assert_eq!(class_of("keyword.control"), Some("syn--keyword"));
        assert_eq!(class_of("nonsense"), None);
    }

    #[test]
    fn every_name_has_a_rule() {
        for (name, class) in NAMES {
            assert!(STYLE.contains(&format!(".{class} ")), "{name} has no rule");
            assert!(
                STYLE.contains(&format!("var(--syntax-{name})")),
                "{name} reads no property"
            );
        }
    }
}
