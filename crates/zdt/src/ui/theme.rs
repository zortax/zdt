//! How a theme reaches the window.
//!
//! Two sheets carry one theme. The component library's tokens go through [`ThemeProvider`], which
//! owns them as a value and writes them itself; this application's own properties — the editor's
//! colours, the syntax captures, the terminal's palette — go through a sheet installed under one
//! name, so replacing it keeps its place in the cascade and switching a theme cannot silently
//! change what beats what.
//!
//! Both are built from the same file, because a person writing a theme should write one block of
//! declarations rather than sort them into two vocabularies. The token schema ignores the
//! properties it has never heard of, and the cascade ignores nothing.

use std::rc::Rc;

use zdt_core::ThemeSource;
use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RenderEffect};
use zgui::{component, view};
use zgui_ui_tokens::prelude::*;

/// The name this application's own theme sheet is installed under.
///
/// One name for every theme: installing under a name that is already there replaces the text
/// without moving the sheet, which is what makes a theme switch a repaint rather than a
/// re-cascade.
pub const SHEET: &str = "zdt-theme";

/// Writes one theme's application-level properties out for the scheme in force.
///
/// Under [`ColorScheme::System`] the light set is written unconditionally and the dark set inside
/// `@media (prefers-color-scheme: dark)`, so the answer comes from the same media query the rest
/// of the document is matched against and follows the desktop with nothing to keep in step. This
/// mirrors what the token provider does with its own sheet.
#[must_use]
pub fn app_sheet(theme: &ThemeSource, scheme: ColorScheme) -> String {
    let mut css = String::new();

    if scheme.wants_light() {
        css.push_str(":root {\n");
        css.push_str(&theme.light);
        css.push_str("\n}\n");
    }

    if scheme.wants_dark() {
        if scheme == ColorScheme::System {
            css.push_str("@media (prefers-color-scheme: dark) {\n:root {\n");
            css.push_str(&theme.dark);
            css.push_str("\n}\n}\n");
        } else {
            css.push_str(":root {\n");
            css.push_str(&theme.dark);
            css.push_str("\n}\n");
        }
    }

    css
}

/// The name the settings' own properties are installed under.
///
/// Between the theme and a person's own sheet: a font named in `config.toml` beats the theme's,
/// and a rule in `user.css` beats both.
pub const SETTINGS_SHEET: &str = "zdt-settings";

/// Writes the settings that are style rather than behaviour into the cascade.
///
/// Fonts and sizes are CSS, so they belong in a sheet rather than being threaded through every
/// component that draws text.
#[must_use]
pub fn settings_sheet(config: &zdt_core::Config) -> String {
    format!(
        ":root {{\n  --zdt-ui-font: \"{ui}\";\n  font-size: {ui_size}px;\n  \
         --tree-width: {tree_width}px;\n}}\n\
         .pane__editor {{\n  --zdt-editor-font: \"{editor}\";\n  font-size: {editor_size}px;\n  \
         tab-size: {tab};\n}}\n",
        ui = config.ui.font,
        ui_size = config.ui.font_size,
        editor = config.editor.font,
        editor_size = config.editor.font_size,
        tab = config.editor.tab_size,
        tree_width = config.tree.width,
    )
}

/// Puts a theme into the window and keeps it there.
///
/// Wraps its children in the token provider, so every component below is themed, and installs the
/// application's own properties beside it. Both follow their signals: writing a different theme
/// into `theme` re-declares both sheets with nothing remounted.
#[component]
pub fn ZdtTheme(
    /// Which theme is in force.
    #[prop(into)]
    theme: Signal<ThemeSource, LocalStorage>,
    /// Which surface it is presented on.
    #[prop(into, default = Signal::stored_local(ColorScheme::System))]
    scheme: Signal<ColorScheme, LocalStorage>,
    /// What the theme applies to.
    children: Children,
) -> impl IntoView {
    // A guard rather than a bare name: the sheet's content is state, so it has to go when this
    // component does, and a cleanup cannot look up the engine it came from.
    let sheet = Stylesheet::install(
        SHEET,
        &app_sheet(&theme.get_untracked(), scheme.get_untracked()),
    );

    let installed = RenderEffect::new(move |previous: Option<()>| {
        let css = app_sheet(&theme.get(), scheme.get());
        // The first run is the install above. Running it again would be a second identical write
        // and a second transcript line for one mount.
        if previous.is_some()
            && let Some(sheet) = sheet.as_ref()
        {
            sheet.replace(&css);
        }
    });
    on_cleanup_local(move || drop(installed));

    let light = Signal::derive_local(move || Theme::light().with_css(&theme.get().light));
    let dark = Signal::derive_local(move || Theme::dark().with_css(&theme.get().dark));

    view! {
        ThemeProvider(scheme = scheme, light = light, dark = dark) {
            {children.into_view_once()}
        }
    }
}

/// The name a person's own style sheet is installed under.
///
/// Last of the three, so it wins: the compiled-in sheet, then the theme, then this.
pub const USER_SHEET: &str = "zdt-user";

/// Installs a person's own style sheet, or takes it away when there is none.
///
/// Replacing under the same name keeps its place in the cascade, which is what makes editing it
/// while the editor runs a repaint rather than a re-cascade.
pub fn install_user_css(css: Option<&str>) {
    match css {
        Some(css) => install_stylesheet(USER_SHEET, css),
        None => remove_stylesheet(USER_SHEET),
    }
}

/// The theme to start with when the configuration names one that is not there.
#[must_use]
pub fn fallback() -> ThemeSource {
    zdt_core::builtin_theme("oldworld").unwrap_or_else(|| {
        // Unreachable while `oldworld` is compiled in, and an empty theme rather than a panic if
        // it ever is not: an unstyled window is still a window the user can quit.
        ThemeSource::new("empty", Rc::from(""), Rc::from(""))
    })
}

#[cfg(test)]
mod tests {
    use zgui_ui_tokens::ColorScheme;

    use super::app_sheet;

    fn theme() -> zdt_core::ThemeSource {
        zdt_core::builtin_theme("oldworld").expect("oldworld is compiled in")
    }

    #[test]
    fn the_system_scheme_writes_both_sets_and_defers_to_the_desktop() {
        let css = app_sheet(&theme(), ColorScheme::System);
        assert!(css.starts_with(":root {"));
        assert!(css.contains("@media (prefers-color-scheme: dark)"));
    }

    #[test]
    fn a_pinned_scheme_writes_no_media_query() {
        // Pinned, nothing in the sheet may change under the interface when the desktop's setting
        // does — that is the whole difference between pinning and deferring.
        for scheme in [ColorScheme::Light, ColorScheme::Dark] {
            let css = app_sheet(&theme(), scheme);
            assert!(
                !css.contains("@media"),
                "{scheme:?} still defers to the desktop"
            );
        }
    }

    #[test]
    fn the_editor_and_the_terminal_are_both_coloured() {
        let css = app_sheet(&theme(), ColorScheme::Dark);
        assert!(css.contains("--editor-bg:"));
        assert!(css.contains("--terminal-background:"));
        assert!(css.contains("--syntax-keyword:"));
    }
}
