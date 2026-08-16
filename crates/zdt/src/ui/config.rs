//! The settings, as a page rather than as a file.
//!
//! Every control here is bound straight to [`crate::settings::Settings`], which is the one signal
//! everything that follows a setting reads. So a change is live the moment it is made — the theme
//! repaints, the tree re-filters, the editor's font changes — with nothing to keep in step, because
//! there is no second copy of the truth to keep it in step *with*.
//!
//! # It writes the file too
//!
//! A panel that only changed the running editor would be a panel whose work is lost at the next
//! start, and one that rewrote the whole file would turn a three-line configuration somebody wrote
//! by hand into two hundred lines they did not. So [`crate::settings::Settings::persist`] writes
//! only the fields that disagree with the defaults, atomically, four hundred milliseconds after the
//! last change — and stamps what it wrote, so the watcher does not read the editor's own write back
//! in and announce it.
//!
//! # Why it is not a fork of the library's components
//!
//! `zgui-ui` ships the whole settings family — the two columns, the page list with roving focus,
//! the groups, the rows that name their control for a screen reader. What it does not ship is this
//! application's density. All of that is `assets/css/settings.css`: the library exposes stable
//! classes for every part, so the compact restyle is a style sheet and no forked components.

use zgui::prelude::*;
use zgui::{component, view};
use zgui_ui::prelude::*;
use zgui_ui_primitives::Binding;
use zgui_ui_primitives::prelude::{PresenceProps, use_presence};

use crate::icons::{self, IconProps};
use crate::settings::Settings as AppSettings;

/// A control's value, read from the settings and written back to them.
///
/// The write goes through [`AppSettings::edit`], which changes the running editor and queues the
/// file — so every control in this panel is live and persistent without either being said twice.
fn bound<T: Clone + PartialEq + 'static>(
    settings: &AppSettings,
    read: impl Fn(&zdt_core::Config) -> T + 'static,
    write: impl Fn(&mut zdt_core::Config, T) + 'static,
) -> Binding<T> {
    let reading = settings.clone();
    let writing = settings.clone();
    Binding::controlled(
        Signal::derive_local(move || reading.with(&read)),
        move |value: T| writing.edit(|config| write(config, value)),
    )
}

/// The same, for a number the settings hold as something other than an `f64`.
///
/// Sliders speak `f64` and the configuration speaks `u32`, `usize` and `f32`. The conversion is
/// here rather than at eleven call sites.
fn number<T: Copy + PartialEq + 'static>(
    settings: &AppSettings,
    read: impl Fn(&zdt_core::Config) -> T + 'static,
    write: impl Fn(&mut zdt_core::Config, f64) + 'static,
) -> Binding<f64>
where
    f64: From<T>,
{
    let reading = settings.clone();
    let writing = settings.clone();
    Binding::controlled(
        Signal::derive_local(move || f64::from(reading.with(&read))),
        move |value: f64| writing.edit(|config| write(config, value)),
    )
}

/// Whether the settings are showing, as a modal.
///
/// A modal rather than a tab, because that is what settings are: something opened, changed, and
/// closed again. The tab remains — `BufferKind::Settings` still renders the same page — for
/// anybody who wants it beside the file whose behaviour they are changing.
#[derive(Clone)]
pub struct ConfigModalState {
    open: zgui::reactive::RwSignal<bool, zgui::reactive::LocalStorage>,
    /// Whose keyboard it borrows, so that closing can give it back.
    workspace: crate::workspace::Workspace,
}

impl ConfigModalState {
    /// Closed.
    #[must_use]
    pub fn new(workspace: crate::workspace::Workspace) -> Self {
        Self {
            open: zgui::reactive::RwSignal::new_local(false),
            workspace,
        }
    }

    /// Whether it is up. Tracked.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.open.get()
    }

    /// Whether it is, without subscribing.
    #[must_use]
    pub fn is_open_untracked(&self) -> bool {
        self.open.get_untracked()
    }

    /// Shows it.
    pub fn open(&self) {
        if !self.open.get_untracked() {
            self.open.set(true);
        }
    }

    /// Puts it away, and gives the keyboard back to the editor.
    ///
    /// The panel takes focus while it is up — `Escape` has to reach it — so something has to hand
    /// it back, or the window is left with the keyboard nowhere and the next motion goes unheard.
    pub fn close(&self) {
        if self.open.get_untracked() {
            self.open.set(false);
        }
        self.workspace.focus_editor();
    }
}

/// Puts the settings modal where every component can find it.
pub fn provide(state: ConfigModalState) {
    zgui::reactive::provide_local_context(state);
}

/// It, from inside a component.
#[must_use]
pub fn use_config_modal() -> Option<ConfigModalState> {
    zgui::reactive::use_local_context::<ConfigModalState>()
}

/// The settings, floating.
#[component]
pub fn ConfigModal() -> impl IntoView {
    use crate::ui::Erase;

    // Nothing to show without the state, which is every test that mounts a piece of the interface
    // without the root above it.
    let Some(state) = use_config_modal() else {
        return view! { box() }.any();
    };
    let surface = NodeRef::new();
    let present = {
        let state = state.clone();
        Signal::derive_local(move || state.is_open())
    };

    view! {
        Presence(
            present = present,
            surface = surface
        ) {
            box(
                class = "config__scrim",
                on:pointer_down = {
                    let state = state.clone();
                    move |_: &mut EventCx<'_, events::PointerDown>| state.close()
                }
            ) {}
            ConfigFloating(surface = surface)
        }
    }
    .any()
}

/// The modal's own box.
///
/// Its own component so that [`use_presence`] runs inside the presence rather than inside an
/// attribute closure called later, which is what gives it a `data-state` to animate on.
#[component]
fn ConfigFloating(
    /// The box itself, whose exit animation says when it may be taken away.
    surface: NodeRef,
) -> impl IntoView {
    let leaving = use_presence();
    let state = use_config_modal();
    let node = NodeRef::new();

    // Escape closes it, which means the panel has to hold the keyboard.
    let claim = zgui::view::time::Timers::current()
        .map(|timers| timers.set_timeout(std::time::Duration::ZERO, move || node.focus()));
    on_cleanup_local(move || drop(claim));

    let on_key = move |event: &mut EventCx<'_, events::KeyDown>| {
        if matches!(event.key, Key::Named(NamedKey::Escape)) {
            if let Some(state) = state.as_ref() {
                state.close();
            }
            event.prevent_default();
        }
        // Everything else belongs to whatever field is being typed into, and to nothing behind.
        event.stop_propagation();
    };

    view! {
        column(
            class = "config__modal",
            node_ref = node,
            tabindex = Focus::Programmatic,
            attr:data-state = move || crate::ui::leaving_state(leaving),
            on:key_down = on_key,
            a11y:role = Role::Dialog,
            a11y:label = "Settings"
        ) {
            box(node_ref = surface, class = "config__modal-body") {
                ConfigPanel()
            }
        }
    }
}

/// The whole page.
#[component]
pub fn ConfigPanel() -> impl IntoView {
    view! {
        Settings(
            class = "config",
            default_page = "appearance",
            label = "Settings"
        ) {
            // The glyph is decoration and the word is the name, so the icons are unlabelled: a
            // reader told "palette, Appearance" has been told the same thing twice.
            SettingsPages(label = "Pages") {
                SettingsPage(value = "appearance") {
                    Icon(icon = icons::PALETTE, class = "config__page-icon")
                    "Appearance"
                }
                SettingsPage(value = "editor") {
                    Icon(icon = icons::PENCIL, class = "config__page-icon")
                    "Editor"
                }
                SettingsPage(value = "language") {
                    Icon(icon = icons::LANGUAGES, class = "config__page-icon")
                    "Language"
                }
                SettingsPage(value = "tree") {
                    Icon(icon = icons::FOLDER_TREE, class = "config__page-icon")
                    "File tree"
                }
                SettingsPage(value = "picker") {
                    Icon(icon = icons::SEARCH, class = "config__page-icon")
                    "Pickers"
                }
                SettingsPage(value = "terminal") {
                    Icon(icon = icons::TERMINAL, class = "config__page-icon")
                    "Terminal"
                }
                SettingsPage(value = "keys") {
                    Icon(icon = icons::KEYBOARD, class = "config__page-icon")
                    "Keys"
                }
            }

            // Each pane reaches for the settings itself rather than being handed them: a pane's
            // children are rebuilt whenever it is shown again, so anything captured here would
            // have to survive being moved out of a closure that runs more than once.
            SettingsPane(value = "appearance") { Appearance() }
            SettingsPane(value = "editor") { Editing() }
            SettingsPane(value = "language") { Language() }
            SettingsPane(value = "tree") { Tree() }
            SettingsPane(value = "picker") { Pickers() }
            SettingsPane(value = "terminal") { Terminal() }
            SettingsPane(value = "keys") { Keys() }
        }
    }
}

/// How the interface looks.
#[component]
fn Appearance() -> impl IntoView {
    let settings = crate::settings::use_settings();
    // The themes the editor knows about: the ones compiled in, and whatever is in the themes
    // directory. Read once — a theme appearing while the panel is open is what the file watcher is
    // for, and a directory listing per frame is not.
    // Stored, because a select's content is built inside a closure that runs again every time the
    // list is opened.
    let themes = StoredValue::new_local(zdt_core::theme::theme_names(
        settings
            .paths()
            .map(zdt_core::config::Paths::themes)
            .as_deref(),
    ));

    let theme = bound(
        &settings,
        |config| config.ui.theme.clone(),
        |config, value| config.ui.theme = value,
    );
    let scheme = bound(
        &settings,
        |config| scheme_name(config.ui.scheme).to_owned(),
        |config, value| config.ui.scheme = scheme_of(&value),
    );
    let font = bound(
        &settings,
        |config| config.ui.font.clone(),
        |config, value| config.ui.font = value,
    );
    let size = number(
        &settings,
        |config| config.ui.font_size,
        |config, value| config.ui.font_size = value as f32,
    );
    let weight = number(
        &settings,
        |config| config.ui.font_weight,
        |config, value| config.ui.font_weight = value as u16,
    );
    let decorations = bound(
        &settings,
        |config| config.ui.client_side_decorations,
        |config, value| config.ui.client_side_decorations = value,
    );
    let notifications = bound(
        &settings,
        |config| config.ui.notifications,
        |config, value| config.ui.notifications = value,
    );
    let timeout = number(
        &settings,
        |config| config.ui.notification_timeout as u32,
        |config, value| config.ui.notification_timeout = value as u64,
    );
    let whichkey = number(
        &settings,
        |config| config.ui.whichkey_delay as u32,
        |config, value| config.ui.whichkey_delay = value as u64,
    );

    view! {
        SettingsGroup {
            SettingsGroupLabel {"Theme"}
            SettingsItem(label = "Theme") {
                // A native chooser rather than the overlay one. What a settings row wants is a
                // list of a dozen names and a value; the overlay `Select` brings a portal, a
                // registered listbox and its own focus scope with it, and every one of those is
                // something else to be wrong on a page that is already inside a floating panel.
                NativeSelect(
                    class = "config__select",
                    value = theme,
                    size = NativeSelectSize::Sm,
                    {..use_settings_item_attrs()}
                ) {
                    {move || themes
                        .get_value()
                        .into_iter()
                        .map(|name| {
                            use crate::ui::Erase;
                            view! {
                                NativeSelectOption(value = name.clone()) {{name}}
                            }
                            .any()
                        })
                        .collect::<Vec<_>>()}
                }
            }
            SettingsItem(
                label = "Surface",
                description = "Follow the desktop, or pin one."
            ) {
                NativeSelect(
                    class = "config__select",
                    value = scheme,
                    size = NativeSelectSize::Sm,
                    {..use_settings_item_attrs()}
                ) {
                    NativeSelectOption(value = "dark") {"Dark"}
                    NativeSelectOption(value = "light") {"Light"}
                    NativeSelectOption(value = "system") {"Follow the desktop"}
                }
            }
        }

        SettingsGroup {
            SettingsGroupLabel {"Type"}
            SettingsItem(label = "Interface font") {
                Input(class = "config__input", value = font, {..use_settings_item_attrs()})
            }
            SettingsItem(label = "Interface size") {
                Number(value = size, min = 8.0, max = 24.0, step = 1.0, unit = "px")
            }
            SettingsItem(
                label = "Interface weight",
                description = "400 is regular, 700 is bold. A font with no such weight is drawn \
                               in the nearest one it has."
            ) {
                Number(value = weight, min = 100.0, max = 900.0, step = 100.0, unit = "")
            }
        }

        SettingsGroup {
            SettingsGroupLabel {"Window"}
            SettingsItem(
                label = "Draw the window frame",
                description = "Off puts the desktop's own title bar back. Takes a restart."
            ) {
                Switch(class = "config__switch", checked = decorations, {..use_settings_item_attrs()})
            }
        }

        SettingsGroup {
            SettingsGroupLabel {"Announcements"}
            SettingsGroupDescription {
                "What the editor says about things nobody asked it about: a language server \
                 starting, a file that would not read."
            }
            SettingsItem(label = "Show announcements") {
                Switch(class = "config__switch", checked = notifications, {..use_settings_item_attrs()})
            }
            SettingsItem(
                label = "How long they stay",
                description = "Zero keeps them until they are dismissed."
            ) {
                Number(value = timeout, min = 0.0, max = 20000.0, step = 500.0, unit = "ms")
            }
            SettingsItem(
                label = "Which-key delay",
                description = "How long a part-typed sequence sits before the hints appear."
            ) {
                Number(value = whichkey, min = 0.0, max = 2000.0, step = 50.0, unit = "ms")
            }
        }
    }
}

/// How the editor behaves.
#[component]
fn Editing() -> impl IntoView {
    let settings = crate::settings::use_settings();
    let font = bound(
        &settings,
        |config| config.editor.font.clone(),
        |config, value| config.editor.font = value,
    );
    let size = number(
        &settings,
        |config| config.editor.font_size,
        |config, value| config.editor.font_size = value as f32,
    );
    let weight = number(
        &settings,
        |config| config.editor.font_weight,
        |config, value| config.editor.font_weight = value as u16,
    );
    let numbers = bound(
        &settings,
        |config| line_numbers_name(config.editor.line_numbers).to_owned(),
        |config, value| config.editor.line_numbers = line_numbers_of(&value),
    );
    let tab_size = number(
        &settings,
        |config| config.editor.tab_size,
        |config, value| config.editor.tab_size = value as u32,
    );
    let expand_tab = bound(
        &settings,
        |config| config.editor.expand_tab,
        |config, value| config.editor.expand_tab = value,
    );
    let scrolloff = number(
        &settings,
        |config| config.editor.scrolloff as u32,
        |config, value| config.editor.scrolloff = value as usize,
    );
    let cursorline = bound(
        &settings,
        |config| config.editor.cursorline,
        |config, value| config.editor.cursorline = value,
    );
    let smooth = bound(
        &settings,
        |config| config.editor.smooth_scroll,
        |config, value| config.editor.smooth_scroll = value,
    );
    let threshold = number(
        &settings,
        |config| config.editor.smooth_scroll_min_lines as f32,
        |config, value| config.editor.smooth_scroll_min_lines = value,
    );

    view! {
        SettingsGroup {
            SettingsGroupLabel {"Type"}
            SettingsItem(label = "Editor font") {
                Input(class = "config__input", value = font, {..use_settings_item_attrs()})
            }
            SettingsItem(label = "Editor size") {
                Number(value = size, min = 8.0, max = 32.0, step = 1.0, unit = "px")
            }
            SettingsItem(
                label = "Editor weight",
                description = "400 is regular, 700 is bold."
            ) {
                Number(value = weight, min = 100.0, max = 900.0, step = 100.0, unit = "")
            }
        }

        SettingsGroup {
            SettingsGroupLabel {"Text"}
            SettingsItem(label = "Line numbers") {
                NativeSelect(
                    class = "config__select",
                    value = numbers,
                    size = NativeSelectSize::Sm,
                    {..use_settings_item_attrs()}
                ) {
                    NativeSelectOption(value = "relative") {"Relative"}
                    NativeSelectOption(value = "absolute") {"Absolute"}
                    NativeSelectOption(value = "none") {"None"}
                }
            }
            SettingsItem(label = "Tab width") {
                Number(value = tab_size, min = 1.0, max = 16.0, step = 1.0, unit = "")
            }
            SettingsItem(
                label = "Insert spaces",
                description = "Off inserts a tab character."
            ) {
                Switch(class = "config__switch", checked = expand_tab, {..use_settings_item_attrs()})
            }
        }

        SettingsGroup {
            SettingsGroupLabel {"The view"}
            SettingsItem(
                label = "Keep lines in view",
                description = "How many lines stay between the caret and the edge."
            ) {
                Number(value = scrolloff, min = 0.0, max = 30.0, step = 1.0, unit = "")
            }
            SettingsItem(label = "Tint the caret's line") {
                Switch(class = "config__switch", checked = cursorline, {..use_settings_item_attrs()})
            }
            SettingsItem(label = "Glide when scrolling") {
                Switch(class = "config__switch", checked = smooth, {..use_settings_item_attrs()})
            }
            SettingsItem(
                label = "Jump under",
                description = "How far the view may move and still jump rather than glide."
            ) {
                Number(value = threshold, min = 0.0, max = 20.0, step = 1.0, unit = "lines")
            }
        }
    }
}

/// What the language servers do.
#[component]
fn Language() -> impl IntoView {
    let settings = crate::settings::use_settings();
    let enabled = bound(
        &settings,
        |config| config.lsp.enabled,
        |config, value| config.lsp.enabled = value,
    );
    let completion = bound(
        &settings,
        |config| config.editor.completion,
        |config, value| config.editor.completion = value,
    );
    let least = number(
        &settings,
        |config| config.editor.completion_min_chars as u32,
        |config, value| config.editor.completion_min_chars = value as usize,
    );
    let docs = bound(
        &settings,
        |config| config.editor.completion_doc,
        |config, value| config.editor.completion_doc = value,
    );
    let delay = number(
        &settings,
        |config| config.editor.completion_doc_delay as u32,
        |config, value| config.editor.completion_doc_delay = value as u64,
    );
    let highlight = bound(
        &settings,
        |config| config.editor.highlight_symbol,
        |config, value| config.editor.highlight_symbol = value,
    );
    let highlight_delay = number(
        &settings,
        |config| config.editor.highlight_symbol_delay as u32,
        |config, value| config.editor.highlight_symbol_delay = value as u64,
    );
    let format_on_save = bound(
        &settings,
        |config| config.editor.format_on_save,
        |config, value| config.editor.format_on_save = value,
    );

    // Which servers are configured, and which of them are answering for the file on screen. Read
    // rather than edited: a server is a dozen fields and a command line, and a panel that offered
    // to edit those badly would be worse than the file that does it well.
    let names = {
        let settings = settings.clone();
        move || {
            settings.with(|config| {
                let mut names: Vec<String> = config.lsp.servers.keys().cloned().collect();
                names.sort_unstable();
                names
            })
        }
    };
    let running = move || {
        zgui::reactive::use_local_context::<crate::language::Language>()
            .and_then(|language| {
                let path = language.current_path()?;
                Some(language.servers_for(&path))
            })
            .unwrap_or_default()
    };

    view! {
        SettingsGroup {
            SettingsGroupLabel {"Language servers"}
            SettingsItem(
                label = "Use language servers",
                description = "Off stops every server and draws no diagnostics."
            ) {
                Switch(class = "config__switch", checked = enabled, {..use_settings_item_attrs()})
            }
            SettingsItem(
                label = "Format when saving",
                description = "Runs the server's formatter before the file is written."
            ) {
                Switch(class = "config__switch", checked = format_on_save, {..use_settings_item_attrs()})
            }
        }

        SettingsGroup {
            SettingsGroupLabel {"Suggestions"}
            SettingsItem(label = "Suggest as you type") {
                Switch(class = "config__switch", checked = completion, {..use_settings_item_attrs()})
            }
            SettingsItem(
                label = "After this many characters",
                description = "One asks as soon as a word starts."
            ) {
                Number(value = least, min = 1.0, max = 5.0, step = 1.0, unit = "")
            }
            SettingsItem(label = "Show documentation beside them") {
                Switch(class = "config__switch", checked = docs, {..use_settings_item_attrs()})
            }
            SettingsItem(
                label = "After resting for",
                description = "Zero opens it at once."
            ) {
                Number(value = delay, min = 0.0, max = 2000.0, step = 50.0, unit = "ms")
            }
        }

        SettingsGroup {
            SettingsGroupLabel {"Under the caret"}
            SettingsItem(
                label = "Mark other uses of the symbol",
                description = "Bands every other place in the file the caret's symbol is used."
            ) {
                Switch(class = "config__switch", checked = highlight, {..use_settings_item_attrs()})
            }
            SettingsItem(label = "After resting for") {
                Number(value = highlight_delay, min = 0.0, max = 2000.0, step = 50.0, unit = "ms")
            }
        }

        SettingsGroup {
            SettingsGroupLabel {"Configured servers"}
            SettingsGroupDescription {
                "Servers are set up in config.toml, where a command line and its arguments belong. \
                 What is running for the file on screen is marked."
            }
            column(class = "config__servers") {
                {move || {
                    let running = running();
                    names()
                        .into_iter()
                        .map(|name| {
                            let on = running.contains(&name);
                            view! {
                                row(
                                    class = "config__server",
                                    attr:data-running = on.then(|| "true".to_owned())
                                ) {
                                    box(class = "config__server-dot") {}
                                    label(class = "nowrap") {{name}}
                                }
                            }
                        })
                        .collect::<Vec<_>>()
                }}
            }
        }
    }
}

/// What the file tree shows.
#[component]
fn Tree() -> impl IntoView {
    let settings = crate::settings::use_settings();
    let open = bound(
        &settings,
        |config| config.tree.open,
        |config, value| config.tree.open = value,
    );
    let width = number(
        &settings,
        |config| config.tree.width,
        |config, value| config.tree.width = value as u32,
    );
    let hidden = bound(
        &settings,
        |config| config.tree.hidden,
        |config, value| config.tree.hidden = value,
    );
    let ignored = bound(
        &settings,
        |config| config.tree.ignored,
        |config, value| config.tree.ignored = value,
    );
    let follow = bound(
        &settings,
        |config| config.tree.follow,
        |config, value| config.tree.follow = value,
    );

    view! {
        SettingsGroup {
            SettingsGroupLabel {"The panel"}
            SettingsItem(label = "Open it with the window") {
                Switch(class = "config__switch", checked = open, {..use_settings_item_attrs()})
            }
            SettingsItem(label = "How wide it is") {
                Number(value = width, min = 140.0, max = 600.0, step = 10.0, unit = "px")
            }
            SettingsItem(
                label = "Follow the editor",
                description = "Moves the tree's caret onto whatever file is being edited."
            ) {
                Switch(class = "config__switch", checked = follow, {..use_settings_item_attrs()})
            }
        }

        SettingsGroup {
            SettingsGroupLabel {"What it shows"}
            SettingsItem(label = "Files beginning with a dot") {
                Switch(class = "config__switch", checked = hidden, {..use_settings_item_attrs()})
            }
            SettingsItem(label = "Files git ignores") {
                Switch(class = "config__switch", checked = ignored, {..use_settings_item_attrs()})
            }
        }
    }
}

/// How the pickers search.
#[component]
fn Pickers() -> impl IntoView {
    let settings = crate::settings::use_settings();
    let preview = bound(
        &settings,
        |config| config.picker.preview,
        |config, value| config.picker.preview = value,
    );
    let rows = number(
        &settings,
        |config| config.picker.max_results as u32,
        |config, value| config.picker.max_results = value as usize,
    );
    let smart_case = bound(
        &settings,
        |config| config.picker.smart_case,
        |config, value| config.picker.smart_case = value,
    );
    let hidden = bound(
        &settings,
        |config| config.picker.hidden,
        |config, value| config.picker.hidden = value,
    );
    let ignored = bound(
        &settings,
        |config| config.picker.ignored,
        |config, value| config.picker.ignored = value,
    );

    view! {
        SettingsGroup {
            SettingsGroupLabel {"The modal"}
            SettingsItem(label = "Show a preview beside the list") {
                Switch(class = "config__switch", checked = preview, {..use_settings_item_attrs()})
            }
            SettingsItem(label = "How many rows at once") {
                Number(value = rows, min = 20.0, max = 2000.0, step = 20.0, unit = "")
            }
        }

        SettingsGroup {
            SettingsGroupLabel {"Searching"}
            SettingsItem(
                label = "Smart case",
                description = "A search with no capitals in it matches either case."
            ) {
                Switch(class = "config__switch", checked = smart_case, {..use_settings_item_attrs()})
            }
            SettingsItem(label = "Look at files beginning with a dot") {
                Switch(class = "config__switch", checked = hidden, {..use_settings_item_attrs()})
            }
            SettingsItem(label = "Look inside files git ignores") {
                Switch(class = "config__switch", checked = ignored, {..use_settings_item_attrs()})
            }
        }
    }
}

/// How terminals are started.
#[component]
fn Terminal() -> impl IntoView {
    let settings = crate::settings::use_settings();
    let shell = bound(
        &settings,
        |config| config.terminal.shell.clone(),
        |config, value| config.terminal.shell = value,
    );
    let width = number(
        &settings,
        |config| (config.terminal.float_width * 100.0).round() as u32,
        |config, value| config.terminal.float_width = (value / 100.0) as f32,
    );
    let height = number(
        &settings,
        |config| (config.terminal.float_height * 100.0).round() as u32,
        |config, value| config.terminal.float_height = (value / 100.0) as f32,
    );
    let scrollback = number(
        &settings,
        |config| config.terminal.scrollback as u32,
        |config, value| config.terminal.scrollback = value as usize,
    );

    view! {
        SettingsGroup {
            SettingsGroupLabel {"The program"}
            SettingsItem(
                label = "Shell",
                description = "Left empty, whatever $SHELL says is used."
            ) {
                Input(class = "config__input", value = shell, placeholder = "$SHELL", {..use_settings_item_attrs()})
            }
            SettingsItem(label = "How many lines of scrollback") {
                Number(value = scrollback, min = 100.0, max = 100000.0, step = 1000.0, unit = "")
            }
        }

        SettingsGroup {
            SettingsGroupLabel {"The floating one"}
            SettingsItem(label = "How wide") {
                Number(value = width, min = 30.0, max = 100.0, step = 5.0, unit = "%")
            }
            SettingsItem(label = "How tall") {
                Number(value = height, min = 30.0, max = 100.0, step = 5.0, unit = "%")
            }
        }
    }
}

/// Which keys are the leaders.
#[component]
fn Keys() -> impl IntoView {
    let settings = crate::settings::use_settings();
    let leader = bound(
        &settings,
        |config| config.keys.leader.clone(),
        |config, value| config.keys.leader = value,
    );
    let local = bound(
        &settings,
        |config| config.keys.local_leader.clone(),
        |config, value| config.keys.local_leader = value,
    );
    let alphabet = bound(
        &settings,
        |config| config.leap.alphabet.clone(),
        |config, value| config.leap.alphabet = value,
    );

    view! {
        SettingsGroup {
            SettingsGroupLabel {"Leaders"}
            SettingsGroupDescription {
                "Written the way the keymap writes them: <Space>, <C-x>, or a bare character."
            }
            SettingsItem(label = "Leader") {
                Input(class = "config__input", value = leader, {..use_settings_item_attrs()})
            }
            SettingsItem(label = "Local leader") {
                Input(class = "config__input", value = local, {..use_settings_item_attrs()})
            }
        }

        SettingsGroup {
            SettingsGroupLabel {"Leaping"}
            SettingsItem(
                label = "Label alphabet",
                description = "The keys labels are handed out from, in order. The earliest are \
                               the ones the fingers are already on."
            ) {
                Input(class = "config__input", value = alphabet, {..use_settings_item_attrs()})
            }
        }

        SettingsGroup {
            SettingsGroupLabel {"The rest"}
            SettingsGroupDescription {
                "Keys themselves are bound in keymap.toml beside config.toml, read after the map \
                 the editor ships with. A row there replaces the shipped row for the same keys, \
                 and `action = false` removes one."
            }
        }
    }
}

/// A number with a slider and the value beside it.
///
/// The library has no number input — only a text field and a slider — and a setting like a tab
/// width wants both: the slider to change it without thinking, and the number to know what it is.
#[component]
fn Number(
    /// The value.
    #[prop(into)]
    value: Binding<f64>,
    /// The smallest it may be.
    min: f64,
    /// The largest.
    max: f64,
    /// How far one keystroke moves it.
    step: f64,
    /// What the number is measured in, shown after it. Empty for a bare count.
    #[prop(into)]
    unit: String,
) -> impl IntoView {
    let showing = Signal::derive_local(move || {
        let held = value.get().unwrap_or_default();
        // Whole numbers as whole numbers: a tab width of `4` should not read as `4.0`.
        if (held - held.round()).abs() < f64::EPSILON {
            format!("{}", held.round() as i64)
        } else {
            format!("{held:.1}")
        }
    });

    view! {
        row(class = "config__number") {
            Slider(
                class = "config__slider",
                value = value,
                min = min,
                max = max,
                step = step,
                {..use_settings_item_attrs()}
            )
            label(class = "config__value nowrap") {
                {move || match unit.as_str() {
                    "" => showing.get(),
                    unit => format!("{} {unit}", showing.get()),
                }}
            }
        }
    }
}

/// How a scheme is written in the settings file.
const fn scheme_name(scheme: zdt_core::config::Scheme) -> &'static str {
    match scheme {
        zdt_core::config::Scheme::Light => "light",
        zdt_core::config::Scheme::Dark => "dark",
        zdt_core::config::Scheme::System => "system",
    }
}

/// The reverse, defaulting to dark for anything unrecognised.
fn scheme_of(name: &str) -> zdt_core::config::Scheme {
    match name {
        "light" => zdt_core::config::Scheme::Light,
        "system" => zdt_core::config::Scheme::System,
        _ => zdt_core::config::Scheme::Dark,
    }
}

/// How a line-numbering choice is written in the settings file.
const fn line_numbers_name(numbers: zdt_core::config::LineNumbers) -> &'static str {
    match numbers {
        zdt_core::config::LineNumbers::Absolute => "absolute",
        zdt_core::config::LineNumbers::Relative => "relative",
        zdt_core::config::LineNumbers::None => "none",
    }
}

/// The reverse.
fn line_numbers_of(name: &str) -> zdt_core::config::LineNumbers {
    match name {
        "absolute" => zdt_core::config::LineNumbers::Absolute,
        "none" => zdt_core::config::LineNumbers::None,
        _ => zdt_core::config::LineNumbers::Relative,
    }
}

#[cfg(test)]
mod tests {
    use super::{line_numbers_name, line_numbers_of, scheme_name, scheme_of};
    use zdt_core::config::{LineNumbers, Scheme};

    #[test]
    fn every_scheme_survives_the_round_trip() {
        // The panel writes the name and the file reads it back, so a name that did not round trip
        // would be a setting that silently reverted the moment it was saved.
        for scheme in [Scheme::Light, Scheme::Dark, Scheme::System] {
            assert_eq!(scheme_of(scheme_name(scheme)), scheme);
        }
    }

    #[test]
    fn every_numbering_survives_it_too() {
        for numbers in [
            LineNumbers::Absolute,
            LineNumbers::Relative,
            LineNumbers::None,
        ] {
            assert_eq!(line_numbers_of(line_numbers_name(numbers)), numbers);
        }
    }

    #[test]
    fn the_names_are_the_ones_the_file_uses() {
        // Which is what makes the panel and a hand-written config.toml agree.
        assert_eq!(scheme_name(Scheme::System), "system");
        assert_eq!(line_numbers_name(LineNumbers::Relative), "relative");

        let written = toml::to_string(&zdt_core::Config::default()).expect("it writes");
        assert!(written.contains("scheme = \"dark\""), "{written}");
        assert!(written.contains("line_numbers = \"relative\""), "{written}");
    }
}
