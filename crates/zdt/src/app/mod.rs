//! What the window contains.
//!
//! Three rows inside the frame: the combined header, the panes, and the status line. Everything
//! below reads the workspace, the settings and the modal layer, all of which are provided here and
//! nowhere else.

pub mod chrome;
pub mod frame;
pub mod statusline;
pub mod theme;

use std::path::PathBuf;

use zdt_core::config::{Paths, Scheme};
use zdt_core::{Project, ThemeSource};
use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RenderEffect, RwSignal};
use zgui::{component, view};
use zgui_ui::prelude::{ToastCorner, ToasterProps};
use zgui_ui_tokens::ColorScheme;

use crate::app::chrome::ChromeProps;
use crate::app::frame::FrameProps;
use crate::app::statusline::StatusLineProps;
use crate::app::theme::{ZdtThemeProps, fallback};
use crate::cmdline::CommandLine;
use crate::cmdline::view::CommandLineProps;
use crate::completion::view::CompletionPopupProps;
use crate::explorer::Explorer;
use crate::explorer::menu::TreeMenuProps;
use crate::explorer::tree::{ExplorerProps, TreeResizeProps};
use crate::git::Git;
use crate::hover::{Hover, HoverPanelProps};
use crate::language::Language;
use crate::picker::Picker;
use crate::picker::view::PickerProps;
use crate::prompt::Prompt;
use crate::prompt::view::PromptProps;
use crate::rename::RenameBoxProps;
use crate::settings::Settings;
use crate::settings::view::ConfigModalProps;
use crate::tabpick::TabPick;
use crate::terminals::Terminals;
use crate::terminals::view::FloatingTerminalProps;
use crate::vim::Vim;
use crate::vim::whichkey::WhichKeyProps;
use crate::workspace::panes::PanesProps;
use crate::workspace::{self, BufferId, Workspace};
use zdt_gitui::GitModalProps;

/// The application.
///
/// Nothing but the toaster. The queue that announcements reach is published *downwards* through
/// the scope tree, so a workbench written beside the toaster could not find it. Everything the
/// editor is made of therefore sits one component further in.
#[component]
pub fn Root(
    /// The directory the editor was opened on.
    project: Project,
    /// The files named on the command line.
    files: Vec<PathBuf>,
) -> impl IntoView {
    view! {
        Toaster(corner = ToastCorner::BottomRight, limit = 4, label = "Notifications") {
            Workbench(project = project, files = files)
        }
    }
}

/// Everything the window contains.
#[component]
fn Workbench(
    /// The directory the editor was opened on.
    project: Project,
    /// The files named on the command line.
    files: Vec<PathBuf>,
) -> impl IntoView {
    let paths = Paths::discover();
    let (settings, problem) = Settings::load(paths.clone());
    crate::settings::provide(settings.clone());

    // Before anything that might announce something, and inside the toaster, which is the only
    // place a queue can be found.
    let notify = crate::notify::Notify::new(settings.clone());
    crate::notify::provide(notify.clone());
    settings.announce_through(notify.clone());

    let space = Workspace::new(project.clone());
    workspace::provide(space.clone());
    if let Some(problem) = problem {
        notify.fail("config.toml did not read", Some(problem));
    }

    let vim = Vim::new(space.clone(), settings.clone());
    zgui::reactive::provide_local_context(vim.clone());

    // The tree's own keys, in front of the base map while the keyboard is in the panel. Always
    // loaded: without them, `j` in the tree would be the editor's `j`.
    apply_tree_keymap(&vim, &notify, paths.as_ref());
    // And the same for every other region that answers keys of its own.
    apply_all_overlays(&vim, &notify, paths.as_ref());

    let explorer = Explorer::new(
        project.root().to_path_buf(),
        settings.with(|config| zdt_core::tree::Filter {
            hidden: config.tree.hidden,
            ignored: config.tree.ignored,
        }),
    );
    crate::explorer::provide(explorer.clone());
    if settings.with(|config| config.tree.open) {
        explorer.toggle();
    }

    crate::prompt::provide(Prompt::new());
    crate::explorer::menu::provide();
    crate::cmdline::provide(CommandLine::new(space.clone()));
    crate::hover::provide(Hover::new());
    crate::rename::provide(crate::rename::Rename::new());
    crate::tabpick::provide(TabPick::new(space.clone()));
    crate::picker::provide(Picker::new(space.clone(), settings.clone()));
    crate::terminals::provide(Terminals::new(space.clone(), settings.clone()));

    // The language servers. Nothing starts until a file that wants one is opened.
    let language = Language::new(space.clone(), settings.clone());
    language.listen();
    crate::language::provide(language.clone());
    let servers = follow_buffers(&language, &space);
    on_cleanup_local(move || drop(servers));

    // After the servers, because the suggestions hold one. A context looked up in a debounce
    // timer is gone. See `tests/context.rs`.
    crate::completion::provide(crate::completion::Completion::new(
        settings.clone(),
        Some(language.clone()),
    ));

    // What git says about the open files, and the panel that shows the rest of it.
    let git = Git::new(space.clone());
    crate::git::provide(git.clone());
    zdt_gitui::provide(crate::git::panel(space.clone()));
    crate::settings::view::provide(crate::settings::view::ConfigModalState::new(space.clone()));

    // The keys leap labels are drawn from, and again whenever the settings change.
    let alphabet = {
        let (settings, vim) = (settings.clone(), vim.clone());
        RenderEffect::new(move |_| {
            let alphabet = settings.with(|config| config.leap.alphabet.clone());
            vim.leaping().set_alphabet(&alphabet);
        })
    };
    on_cleanup_local(move || drop(alphabet));

    // The tree keeps up with the editor, and with the settings.
    let following_buffer = follow_buffer(&explorer, &space, &settings);
    on_cleanup_local(move || drop(following_buffer));
    let following_filter = follow_filter(&explorer, &settings);
    on_cleanup_local(move || drop(following_filter));

    // A person's own keymap, read after the shipped one so a row in it replaces the shipped row
    // for the same keys.
    if let Some(paths) = paths.as_ref() {
        apply_keymap(&vim, &notify, paths, &settings);
    }

    // The theme follows the settings, and both follow the files on disk.
    let theme: RwSignal<ThemeSource, LocalStorage> = RwSignal::new_local(read_theme(&settings));
    let following = {
        let settings = settings.clone();
        RenderEffect::new(move |_| {
            let next = read_theme(&settings);
            if theme.with_untracked(|held| held.name != next.name) {
                theme.set(next);
            }
        })
    };
    on_cleanup_local(move || drop(following));

    let scheme = {
        let settings = settings.clone();
        Signal::derive_local(move || match settings.with(|config| config.ui.scheme) {
            Scheme::Light => ColorScheme::Light,
            Scheme::Dark => ColorScheme::Dark,
            Scheme::System => ColorScheme::System,
        })
    };

    // The settings that are style, in the cascade between the theme and a person's own sheet.
    let styling = {
        let settings = settings.clone();
        RenderEffect::new(move |_| {
            let css = settings.with(crate::app::theme::settings_sheet);
            install_stylesheet(crate::app::theme::SETTINGS_SHEET, &css);
        })
    };
    on_cleanup_local(move || drop(styling));

    // A person's own sheet, last of the three.
    if let Some(paths) = paths.as_ref() {
        crate::app::theme::install_user_css(
            zdt_core::config::read_optional(&paths.user_css()).as_deref(),
        );
    }

    // What a change on disk does. Held for the window's life; dropping it stops the watching.
    let watcher = paths.as_ref().and_then(|paths| {
        let (settings, notify, vim, held) =
            (settings.clone(), notify.clone(), vim.clone(), paths.clone());
        crate::reload::watch(paths, move || {
            reload(&settings, &notify, &vim, &held, theme);
        })
    });
    on_cleanup_local(move || drop(watcher));

    for file in files {
        crate::files::open_argument(&space, &file);
    }

    view! {
        ZdtTheme(theme = theme, scheme = scheme) {
            Frame {
                // The tree runs the whole height of the window and the buffer line sits over the
                // panes alone: a tab bar reaching across a file tree says the tabs belong to the
                // tree, and they do not.
                row(class = "frame__body") {
                    Explorer()
                    TreeResize()
                    column(class = "workarea") {
                        Chrome()
                        Panes()
                    }
                }
                HoverPanel()
                CompletionPopup()
                RenameBox()
                GitModal()
                ConfigModal()
                TreeMenu()
                FloatingTerminal()
                Picker()
                Prompt()
                WhichKey()
                CommandLine()
                StatusLine()
            }
        }
    }
}

/// The theme the settings name, or the one the editor falls back to.
fn read_theme(settings: &Settings) -> ThemeSource {
    let name = settings.with(|config| config.ui.theme.clone());
    let directory = settings.paths().map(zdt_core::config::Paths::themes);
    zdt_core::theme::resolve_theme(directory.as_deref(), &name).unwrap_or_else(|| {
        tracing::warn!("no theme called {name}; using the built-in one");
        fallback()
    })
}

/// Tells the language layer about every file that is opened.
///
/// This watches the buffer list. A buffer arrives from the picker, the tree, the command line and
/// the command line arguments. One place sees all four.
fn follow_buffers(language: &Language, workspace: &Workspace) -> RenderEffect<Vec<BufferId>> {
    let (language, workspace) = (language.clone(), workspace.clone());
    RenderEffect::new(move |previous: Option<Vec<BufferId>>| {
        let order = workspace.order();
        let previous = previous.unwrap_or_default();

        for id in &order {
            if !previous.contains(id) {
                language.opened(*id);
                if let Some(git) = zgui::reactive::use_local_context::<crate::git::Git>() {
                    git.refresh(*id);
                }
            }
        }
        for id in &previous {
            if !order.contains(id)
                && let Some(path) = workspace
                    .buffer_untracked(*id)
                    .and_then(|buffer| buffer.path)
            {
                language.closed(&path);
            }
        }
        order
    })
}

/// Moves the tree's caret onto whatever the editor is showing.
///
/// Only while the panel is open, because opening the way to a file reads every directory along it
/// and there is no reason to pay that for a panel nobody is looking at.
fn follow_buffer(explorer: &Explorer, space: &Workspace, settings: &Settings) -> RenderEffect<()> {
    let (explorer, space, settings) = (explorer.clone(), space.clone(), settings.clone());
    RenderEffect::new(move |_| {
        let path = space.current_buffer().and_then(|buffer| buffer.path);
        if !explorer.is_open() || !settings.with(|config| config.tree.follow) {
            return;
        }
        // Never while the keyboard is in the panel. A caret that jumps out from under somebody
        // walking the tree is worse than one that is a file behind.
        if explorer.is_focused_untracked() {
            return;
        }
        if let Some(path) = path {
            explorer.reveal(&path);
        }
    })
}

/// Keeps what the tree shows in step with the settings.
fn follow_filter(explorer: &Explorer, settings: &Settings) -> RenderEffect<()> {
    let (explorer, settings) = (explorer.clone(), settings.clone());
    RenderEffect::new(move |_| {
        let wanted = settings.with(|config| zdt_core::tree::Filter {
            hidden: config.tree.hidden,
            ignored: config.tree.ignored,
        });
        if explorer.filter() != wanted {
            explorer.set_filter(wanted);
        }
    })
}

/// The file tree's keys: the shipped ones, then a person's own on top.
fn apply_tree_keymap(vim: &Vim, notify: &crate::notify::Notify, paths: Option<&Paths>) {
    let theirs = paths.and_then(|paths| zdt_core::config::read_optional(&paths.tree_keymap()));
    if let Err(problems) = vim.load_overlay("tree", crate::assets::TREE_KEYMAP, theirs.as_deref()) {
        notify.fail("keymap-tree.toml", Some(problems.join("; ")));
    }
}

/// A region's own keys: the shipped ones, then a person's own on top.
///
/// Every overlay has the same shape: a shipped file, and an optional one beside it in the
/// configuration directory. So one function loads them all.
fn apply_overlay(
    vim: &Vim,
    notify: &crate::notify::Notify,
    paths: Option<&Paths>,
    region: &str,
    shipped: &str,
    file: &str,
) {
    let theirs = paths.and_then(|paths| zdt_core::config::read_optional(&paths.root.join(file)));
    if let Err(problems) = vim.load_overlay(region, shipped, theirs.as_deref()) {
        notify.fail(file.to_owned(), Some(problems.join("; ")));
    }
}

/// Reads a person's keymap on top of the shipped one, saying what did not read.
fn apply_keymap(vim: &Vim, notify: &crate::notify::Notify, paths: &Paths, settings: &Settings) {
    let Some(text) = zdt_core::config::read_optional(&paths.keymap()) else {
        return;
    };
    let leaders = leaders_from(settings);
    if let Err(problems) = vim.merge_keymap(&text, leaders) {
        notify.fail("keymap.toml", Some(problems.join("; ")));
    }
}

/// What `<Leader>` and `<LocalLeader>` stand for, as the settings say.
fn leaders_from(settings: &Settings) -> zdt_vim::Leaders {
    let (leader, local) =
        settings.with(|config| (config.keys.leader.clone(), config.keys.local_leader.clone()));
    let default = zdt_vim::Leaders::default();
    let one = |text: &str, fallback| {
        zdt_vim::notation::parse(text, default)
            .ok()
            .and_then(|chords| chords.first().copied())
            .unwrap_or(fallback)
    };
    zdt_vim::Leaders {
        leader: one(&leader, default.leader),
        local: one(&local, default.local),
    }
}

/// Everything a change on disk brings, put where the interface reads it.
///
/// The files are read on a worker; only the writing happens here. A settings file that does not
/// read leaves the old settings in place and says so, because half-applied configuration is worse
/// than none.
fn reload(
    settings: &Settings,
    notify: &crate::notify::Notify,
    vim: &Vim,
    paths: &Paths,
    theme: RwSignal<ThemeSource, LocalStorage>,
) {
    let (settings, notify, vim, paths) =
        (settings.clone(), notify.clone(), vim.clone(), paths.clone());

    let task = zgui::task::spawn_local(async move {
        let reading = paths.clone();
        let reloaded = zgui::task::blocking(move || crate::reload::read(&reading)).await;

        // What this editor wrote itself, coming back around through the watcher. Applying it
        // would be applying what is already applied, and saying so would be announcing somebody's
        // own keystroke back at them.
        if settings.wrote(reloaded.config_text.as_deref()) {
            return;
        }

        for problem in &reloaded.problems {
            notify.fail("configuration", Some(problem.clone()));
        }
        if let Some(config) = reloaded.config {
            settings.replace(config);
        }

        // The keymap is rebuilt from the shipped one, and never layered onto what is already
        // there. A row somebody took out of their file has to come back.
        vim.reset_keymap();
        if let Some(text) = reloaded.keymap
            && let Err(problems) = vim.merge_keymap(&text, leaders_from(&settings))
        {
            notify.fail("keymap.toml", Some(problems.join("; ")));
        }

        apply_tree_keymap(&vim, &notify, Some(&paths));
        apply_all_overlays(&vim, &notify, Some(&paths));
        theme.set(read_theme(&settings));
        crate::app::theme::install_user_css(reloaded.user_css.as_deref());

        if reloaded.problems.is_empty() {
            notify.say("configuration reloaded");
        }
    });
    // The task belongs to the root's owner and is cancelled with the window.
    std::mem::forget(task);
}

/// Every region's keymap overlay, loaded.
///
/// One call site for all of them, so a region added later is one row here. Three places that have
/// to agree would be three places to forget.
fn apply_all_overlays(vim: &Vim, notify: &crate::notify::Notify, paths: Option<&Paths>) {
    for (region, shipped, file) in crate::assets::OVERLAYS {
        apply_overlay(vim, notify, paths, region, shipped, file);
    }
}
