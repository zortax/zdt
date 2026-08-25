//! The commit modal.
//!
//! One card over everything: the files a commit would take, a drafted message to read and
//! change, and the buttons that actually commit. The scan and the draft start when the modal
//! opens; the fields fill as the answers land, and nothing touches the repository until a
//! person presses commit.
//!
//! The draft never touches typing: only a field standing empty takes the model's words, so
//! clearing a field and pressing regenerate fills it again and everything else stays.

use zdt_icons::{self as icons, IconProps};
use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};
use zgui::{component, view};
use zgui_ui::prelude::*;

use crate::use_agent;

/// The modal. Mounted for good; an open [`Committing`](crate::Committing) shows it.
///
/// `subject` is the message field's node, registered as the commit spot's focus sink.
// The list macro takes a closure by construction, so the one it is handed here is not redundant.
#[allow(clippy::redundant_closure)]
#[component]
pub fn CommitModal(
    /// The message field, for whoever gives it the keyboard.
    subject: NodeRef,
) -> impl IntoView {
    let agent = use_agent();
    // The portal's children are an `Fn` closure: everything they capture must be `Copy`, so
    // the state rides in a signal and every view closure pulls it out.
    let held: RwSignal<Option<crate::AgentUi>, LocalStorage> =
        RwSignal::new_local(Some(agent.clone()));

    let body = NodeRef::new();
    let branch = NodeRef::new();

    // The fields' text. Bound both ways: typing writes here, and a write here fills the field.
    let subject_text: RwSignal<String, LocalStorage> = RwSignal::new_local(String::new());
    let body_text: RwSignal<String, LocalStorage> = RwSignal::new_local(String::new());
    let branch_text: RwSignal<String, LocalStorage> = RwSignal::new_local(String::new());

    // Choosing files: the edit toggle, and the paths taken out while it is on.
    let editing: RwSignal<bool, LocalStorage> = RwSignal::new_local(false);
    let excluded: RwSignal<std::collections::HashSet<String>, LocalStorage> =
        RwSignal::new_local(std::collections::HashSet::new());

    // Everything the view reads goes through `held`, so every closure below captures only
    // `Copy` values and the portal can call its children again.
    let shown = move || {
        held.with_untracked(|agent| agent.as_ref().map(crate::AgentUi::committing))
            .flatten()
            .is_none()
            .then(|| "none".to_owned())
    };

    // A fresh opening clears everything: the last modal's words belong to the last modal.
    let opening = {
        let agent = agent.clone();
        zgui::reactive::RenderEffect::new(move |was: Option<Option<crate::Committing>>| {
            let now = agent.committing();
            if now.is_some() && was.flatten() != now {
                subject_text.set(String::new());
                body_text.set(String::new());
                branch_text.set(String::new());
                editing.set(false);
                excluded.set(std::collections::HashSet::new());
            }
            now
        })
    };
    on_cleanup_local(move || drop(opening));

    // The draft lands in whichever fields stand empty; anything a person holds stays theirs.
    let filling = {
        let agent = agent.clone();
        zgui::reactive::RenderEffect::new(move |_| {
            let Some(opened) = agent.committing() else {
                return;
            };
            let Some(draft) = agent.client().commit_draft() else {
                return;
            };
            if draft.thread != opened.thread {
                return;
            }
            let mut landed = Vec::new();
            if subject_text.with_untracked(String::is_empty) {
                subject_text.set(draft.subject.clone());
                landed.push((subject, draft.subject.len()));
            }
            if body_text.with_untracked(String::is_empty) {
                body_text.set(draft.body.clone());
                landed.push((body, draft.body.len()));
            }
            if branch_text.with_untracked(String::is_empty) {
                branch_text.set(draft.branch.clone());
                landed.push((branch, draft.branch.len()));
            }
            // The carets follow to each filled text's end once the values have taken: a written
            // value applies on a later frame, and typing must continue after the draft, never
            // before it.
            if !landed.is_empty() && zgui::view::time::Timers::current().is_some() {
                let handle = zgui::view::time::set_timeout(
                    std::time::Duration::from_millis(30),
                    move || {
                        for (field, end) in &landed {
                            field.set_selection(*end..*end);
                        }
                    },
                );
                std::mem::forget(handle);
            }
        })
    };
    on_cleanup_local(move || drop(filling));

    // The files the scan found, for the list and the totals.
    fn files_of(
        held: RwSignal<Option<crate::AgentUi>, LocalStorage>,
    ) -> Option<Vec<zdt_agent::change::FileStat>> {
        let agent = held.with_untracked(Clone::clone)?;
        let opened = agent.committing()?;
        let (thread, files) = agent.client().commit_files()?;
        (thread == opened.thread).then_some(files)
    }
    let file_rows = move || files_of(held).unwrap_or_default();
    let totals = move || {
        let Some(files) = files_of(held) else {
            return (String::new(), String::new(), String::new());
        };
        let all = files.len();
        let chosen: Vec<_> = if editing.get() {
            excluded.with(|out| {
                files
                    .iter()
                    .filter(|file| !out.contains(&file.path))
                    .collect()
            })
        } else {
            files.iter().collect()
        };
        let added: u32 = chosen.iter().map(|file| file.added).sum();
        let removed: u32 = chosen.iter().map(|file| file.removed).sum();
        let count = if chosen.len() == all {
            format!("{all} file{}", if all == 1 { "" } else { "s" })
        } else {
            format!("{} of {all} files", chosen.len())
        };
        (count, format!("+{added}"), format!("\u{2212}{removed}"))
    };
    let totals_word = {
        let totals = totals.clone();
        move || totals().0
    };
    let totals_added = {
        let totals = totals.clone();
        move || totals().1
    };
    let totals_removed = {
        let totals = totals.clone();
        move || totals().2
    };
    let scanning = move || files_of(held).is_some().then(|| "none".to_owned());
    let empty_tree = move || {
        files_of(held)
            .is_none_or(|files| !files.is_empty())
            .then(|| "none".to_owned())
    };

    // Whether the model is still writing, for the spinner and the regenerate button.
    fn waiting_of(held: RwSignal<Option<crate::AgentUi>, LocalStorage>) -> bool {
        held.with_untracked(Clone::clone).is_some_and(|agent| {
            agent.committing().is_some()
                && agent.client().commit_draft().is_none()
                && files_of(held).is_none_or(|files| !files.is_empty())
        })
    }
    let drafting = move || (!waiting_of(held)).then(|| "none".to_owned());
    let drafted = move || {
        let done = held
            .with_untracked(Clone::clone)
            .is_some_and(|agent| agent.client().commit_draft().is_some());
        (!done).then(|| "none".to_owned())
    };

    let commit_word = move || {
        let pushes = held
            .with_untracked(Clone::clone)
            .and_then(|agent| agent.committing())
            .is_some_and(|opened| opened.push);
        if pushes {
            "Commit and push".to_owned()
        } else {
            "Commit".to_owned()
        }
    };

    let close = move || {
        if let Some(agent) = held.with_untracked(Clone::clone) {
            agent.close_commit();
        }
    };
    // What the commit takes: everything while the toggle is off, the checked files while it is
    // on. Nothing when every box is cleared, which the callers refuse.
    let chosen_paths = move || -> Option<Vec<String>> {
        if !editing.get_untracked() || excluded.with_untracked(std::collections::HashSet::is_empty)
        {
            return Some(Vec::new());
        }
        let files = files_of(held).unwrap_or_default();
        let paths: Vec<String> = excluded.with_untracked(|out| {
            files
                .iter()
                .filter(|file| !out.contains(&file.path))
                .map(|file| file.path.clone())
                .collect()
        });
        (!paths.is_empty()).then_some(paths)
    };
    let commit_here = move || {
        let Some(agent) = held.with_untracked(Clone::clone) else {
            return;
        };
        let Some(paths) = chosen_paths() else {
            agent.host().say("choose at least one file first");
            return;
        };
        agent.commit_now(
            &subject_text.get_untracked(),
            &body_text.get_untracked(),
            "",
            paths,
        );
    };
    let commit_branch = move || {
        let Some(agent) = held.with_untracked(Clone::clone) else {
            return;
        };
        let name = branch_text.get_untracked();
        if name.trim().is_empty() {
            agent.host().say("the new branch needs a name");
            return;
        }
        let Some(paths) = chosen_paths() else {
            agent.host().say("choose at least one file first");
            return;
        };
        agent.commit_now(
            &subject_text.get_untracked(),
            &body_text.get_untracked(),
            &name,
            paths,
        );
    };
    // A fresh draft for whatever stands empty. The answer fills only empty fields, so pressing
    // this with everything held changes nothing.
    let regenerate = move || {
        let Some(agent) = held.with_untracked(Clone::clone) else {
            return;
        };
        if let Some(opened) = agent.committing() {
            agent.client().draft_commit(opened.thread);
        }
    };

    // The card's keys: escape closes, control-enter commits, control-b takes the branch.
    let on_key = move |cx: &mut EventCx<'_, events::KeyDown>| {
        use zgui::vocab::{Key, NamedKey};
        match &cx.key {
            Key::Named(NamedKey::Escape) => {
                close();
                cx.prevent_default();
                cx.stop_propagation();
            }
            Key::Named(NamedKey::Enter) if cx.modifiers.control() => {
                commit_here();
                cx.prevent_default();
                cx.stop_propagation();
            }
            Key::Character(typed) if cx.modifiers.control() && typed.as_str() == "b" => {
                commit_branch();
                cx.prevent_default();
                cx.stop_propagation();
            }
            _ => {}
        }
    };
    // Enter in the one-line fields walks on; the description keeps its newlines.
    let subject_key = move |cx: &mut EventCx<'_, events::KeyDown>| {
        use zgui::vocab::{Key, NamedKey};
        if matches!(
            cx.key,
            Key::Named(NamedKey::Enter) | Key::Named(NamedKey::Tab)
        ) && !cx.modifiers.control()
        {
            body.focus();
            cx.prevent_default();
            cx.stop_propagation();
        }
    };
    let body_key = move |cx: &mut EventCx<'_, events::KeyDown>| {
        use zgui::vocab::{Key, NamedKey};
        if matches!(cx.key, Key::Named(NamedKey::Tab)) && !cx.modifiers.control() {
            branch.focus();
            cx.prevent_default();
            cx.stop_propagation();
        }
    };
    let branch_key = move |cx: &mut EventCx<'_, events::KeyDown>| {
        use zgui::vocab::{Key, NamedKey};
        match &cx.key {
            Key::Named(NamedKey::Tab) if !cx.modifiers.control() => {
                subject.focus();
                cx.prevent_default();
                cx.stop_propagation();
            }
            Key::Named(NamedKey::Enter) if !cx.modifiers.control() => {
                commit_branch();
                cx.prevent_default();
                cx.stop_propagation();
            }
            _ => {}
        }
    };

    let taken = move |_: &mut EventCx<'_, events::FocusIn>| {
        if let Some(agent) = held.with_untracked(Clone::clone) {
            agent.host().took_keyboard();
        }
    };

    let dismiss = move |event: &mut EventCx<'_, events::PointerDown>| {
        event.stop_propagation();
        close();
    };
    let keep = move |event: &mut EventCx<'_, events::PointerDown>| {
        event.stop_propagation();
    };
    let press_cancel = move |event: &mut EventCx<'_, events::PointerDown>| {
        event.stop_propagation();
        close();
    };
    let press_branch = move |event: &mut EventCx<'_, events::PointerDown>| {
        event.stop_propagation();
        commit_branch();
    };
    let press_commit = move |event: &mut EventCx<'_, events::PointerDown>| {
        event.stop_propagation();
        commit_here();
    };
    let press_regenerate = move |event: &mut EventCx<'_, events::PointerDown>| {
        event.stop_propagation();
        regenerate();
    };
    let press_edit = move |event: &mut EventCx<'_, events::PointerDown>| {
        event.stop_propagation();
        editing.update(|held| *held = !*held);
    };
    let edit_on = move || editing.get().then(|| "true".to_owned());

    view! {
        Portal {
            box(class = "agent-commit__backdrop", style:display = shown, on:pointer_down = dismiss) {
                column(
                    class = "agent-commit",
                    on:pointer_down = keep,
                    on:key_down = on_key,
                    on:focus_in = taken
                ) {
                    row(class = "agent-commit__head") {
                        Icon(icon = icons::GIT_COMMIT, class = "icon--sm")
                        label(class = "agent-commit__title nowrap") {"Commit"}
                        box(class = "fill") {}
                        label(class = "muted nowrap") {{totals_word}}
                        label(class = "agent-added nowrap") {{totals_added}}
                        label(class = "agent-removed nowrap") {{totals_removed}}
                        control(
                            class = "agent-commit__edit",
                            tabindex = Focus::Programmatic,
                            a11y:label = "Choose the files",
                            attr:data-on = edit_on,
                            on:pointer_down = press_edit
                        ) {
                            Icon(icon = icons::PENCIL, class = "icon--xs")
                        }
                    }
                    scroll(class = "agent-commit__files") {
                        label(class = "agent-commit__note muted", style:display = scanning) {
                            "reading the tree\u{2026}"
                        }
                        label(class = "agent-commit__note muted", style:display = empty_tree) {
                            "nothing to commit"
                        }
                        // Keyed by the whole row, because a row keyed by path alone would keep
                        // its first counts through a rescan.
                        for file in move || file_rows(), key = |file: &zdt_agent::change::FileStat| {
                            format!("{}:{}:{}", file.path, file.added, file.removed)
                        } {
                            FileRow(file = file, editing = editing, excluded = excluded)
                        }
                    }
                    // One card for the message: the subject line, a rule, and the body under it.
                    column(class = "agent-commit__msgbox") {
                        row(class = "agent-commit__fieldrow") {
                            Icon(icon = icons::PENCIL, class = "icon--xs agent-commit__mark")
                            Input(
                                class = "agent-commit__input",
                                node_ref = subject,
                                value = Binding::from(subject_text),
                                placeholder = "Commit message",
                                label = "Commit message",
                                on:key_down = subject_key
                            )
                        }
                        box(class = "agent-commit__rule") {}
                        row(class = "agent-commit__fieldrow agent-commit__fieldrow--tall") {
                            Icon(icon = icons::TYPE, class = "icon--xs agent-commit__mark")
                            Textarea(
                                class = "agent-commit__input agent-commit__input--tall",
                                node_ref = body,
                                value = Binding::from(body_text),
                                placeholder = "Description (optional)",
                                label = "Commit description",
                                on:key_down = body_key
                            )
                        }
                    }
                    row(class = "agent-commit__msgbox agent-commit__fieldrow") {
                        Icon(icon = icons::GIT_BRANCH_PLUS, class = "icon--xs agent-commit__mark")
                        Input(
                            class = "agent-commit__input",
                            node_ref = branch,
                            value = Binding::from(branch_text),
                            placeholder = "feat/new-branch-name",
                            label = "New branch name",
                            on:key_down = branch_key
                        )
                    }
                    row(class = "agent-commit__foot") {
                        row(class = "agent-commit__hint", style:display = drafting) {
                            Icon(icon = icons::LOADER_CIRCLE, class = "icon--xs zdt-spin")
                            label(class = "muted nowrap") {"drafting a message\u{2026}"}
                        }
                        control(
                            class = "agent-commit__ghost",
                            tabindex = Focus::Programmatic,
                            a11y:label = "Draft again for empty fields",
                            style:display = drafted,
                            on:pointer_down = press_regenerate
                        ) {
                            Icon(icon = icons::REFRESH_CW, class = "icon--xs")
                            label(class = "nowrap") {"regenerate"}
                        }
                        box(class = "fill") {}
                        control(
                            class = "agent-commit__button",
                            tabindex = Focus::Programmatic,
                            a11y:label = "Cancel",
                            on:pointer_down = press_cancel
                        ) {
                            label(class = "nowrap") {"Cancel"}
                        }
                        control(
                            class = "agent-commit__button",
                            tabindex = Focus::Programmatic,
                            a11y:label = "Commit to a new branch",
                            on:pointer_down = press_branch
                        ) {
                            Icon(icon = icons::GIT_BRANCH_PLUS, class = "icon--xs")
                            label(class = "nowrap") {"Commit to new branch"}
                        }
                        control(
                            class = "agent-commit__button agent-commit__button--first",
                            tabindex = Focus::Programmatic,
                            a11y:label = "Commit",
                            on:pointer_down = press_commit
                        ) {
                            Icon(icon = icons::GIT_COMMIT, class = "icon--xs")
                            label(class = "nowrap") {{commit_word}}
                        }
                    }
                }
            }
        }
    }
}

/// One file the commit would take: a checkbox while files are being chosen, the path, and its
/// counts or the word binary.
#[component]
fn FileRow(
    /// The file.
    file: zdt_agent::change::FileStat,
    /// Whether the checkboxes are on.
    editing: RwSignal<bool, LocalStorage>,
    /// The paths taken out.
    excluded: RwSignal<std::collections::HashSet<String>, LocalStorage>,
) -> impl IntoView {
    use zdt_view::Erase;
    let path = file.path.clone();
    let taken = {
        let path = path.clone();
        move || excluded.with(|out| !out.contains(&path))
    };
    // The library's checkbox, held from outside: what it shows is the set, and operating it
    // moves the path in or out of the set.
    let checked = {
        let reading = path.clone();
        let writing = path.clone();
        Binding::controlled(
            Signal::derive_local(move || {
                Checked::from(excluded.with(|out| !out.contains(&reading)))
            }),
            move |now: Checked| {
                let put_back = now == Checked::Yes;
                excluded.update(|out| {
                    if put_back {
                        out.remove(&writing);
                    } else {
                        out.insert(writing.clone());
                    }
                });
            },
        )
    };
    let box_shown = move || (!editing.get()).then(|| "none".to_owned());
    let dimmed = {
        let taken = taken.clone();
        move || (editing.get() && !taken()).then(|| "true".to_owned())
    };
    // A press on the box itself stays the box's: the checkbox toggles through its binding, and
    // the row handler must not toggle it straight back.
    let box_keeps = move |event: &mut EventCx<'_, events::PointerDown>| {
        event.stop_propagation();
    };
    // A press anywhere else on the row is a press on its box.
    let toggle = {
        let path = path.clone();
        move |event: &mut EventCx<'_, events::PointerDown>| {
            if !editing.get_untracked() {
                return;
            }
            event.stop_propagation();
            excluded.update(|out| {
                if !out.remove(&path) {
                    out.insert(path.clone());
                }
            });
        }
    };
    let counts = if file.binary {
        view! { label(class = "muted nowrap") {"binary"} }.any()
    } else {
        view! {
            row(class = "agent-commit__counts") {
                label(class = "agent-added nowrap") {{format!("+{}", file.added)}}
                label(class = "agent-removed nowrap") {{format!("\u{2212}{}", file.removed)}}
            }
        }
        .any()
    };
    view! {
        row(class = "agent-commit__file", attr:data-out = dimmed, on:pointer_down = toggle) {
            box(
                class = "agent-commit__check",
                style:display = box_shown,
                on:pointer_down = box_keeps
            ) {
                Checkbox(checked = checked, a11y:label = file.path.clone())
            }
            label(class = "agent-commit__path nowrap") {{file.path}}
            box(class = "fill") {}
            {counts}
        }
    }
}
