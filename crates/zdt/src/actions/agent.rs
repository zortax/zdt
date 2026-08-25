//! The agent commands.

use crate::vim::Vim;
use crate::workspace::Workspace;

/// Carries out `leaf`, the part of an `agent.*` action after the dot.
pub(super) fn run(workspace: &Workspace, vim: &Vim, leaf: &str) {
    let Some(agent) = zdt_agentui::try_use_agent() else {
        workspace.say("the agent surface is not installed");
        return;
    };
    if !crate::settings::use_settings().with_untracked(|config| config.agent.enabled) {
        workspace.say("the agent surface is off; [agent] enabled = true turns it on");
        return;
    }

    match leaf {
        "down" => agent.step(1),
        "up" => agent.step(-1),
        // Half a screenful, in rows. The list has one row height, and the key means "a good way
        // down".
        "half_down" => agent.step(10),
        "half_up" => agent.step(-10),
        "first" => agent.to_top(),
        "last" => agent.to_bottom(),

        "open" => agent.open_at(agent.at()),
        "toggle" => {
            agent.toggle_screen();
            vim.reset();
        }
        "sidebar" => {
            agent.toggle_sidebar();
            vim.reset();
        }
        "focus" => {
            agent.set_open(true);
            agent.to_list();
            agent.host().focus_agent();
            vim.reset();
        }
        "compose" => agent.compose(),
        "leave" => {
            if agent.screen() == zdt_agentui::Screen::Agent {
                agent.toggle_screen();
            } else {
                agent.host().leave();
            }
        }

        "new" => new_thread(&agent, workspace),
        "worktree" => worktree(&agent, workspace),
        "stop" => agent.interrupt(),
        "delete" => delete(&agent),
        "find" => find(&agent),
        "filter" => agent.focus_filter(),

        // The lifecycle overlays, each with its reverse.
        "pin" => agent.pin_toggle(),
        "pin_up" => agent.pin_move(-1),
        "pin_down" => agent.pin_move(1),
        "settle" => agent.settle_toggle(),
        "archive" => agent.archive_toggle(),
        "archived" => agent.toggle_archived(),
        "unread" => agent.unread_toggle(),
        "snooze" => snooze(&agent, workspace),
        "rename" => agent.rename_prompt(),
        "retitle" => {
            if let Some(shell) = agent.caret_shell() {
                agent.client().rename(shell.id, String::new());
                workspace.say("making a name up\u{2026}");
            }
        }

        // The diff review surface, and git around it.
        "review" => agent.review_thread(),
        "revert" => revert(&agent),
        "commit" => agent.open_commit(false),
        "commit_push" => agent.open_commit(true),
        "review_down" => agent.review_step(1),
        "review_up" => agent.review_step(-1),
        "review_open" => agent.review_open_file(),
        "review_split" => agent.toggle_review_split(),
        "review_whitespace" => agent.toggle_review_ws(),
        "review_close" => {
            agent.close_review();
            agent.to_chat();
            vim.reset();
        }
        "daemon_stop" => {
            agent.client().shutdown_daemon();
            workspace.say("asked the agent daemon to stop");
        }

        // The timeline scrolls by mouse alone; these keys only walk an open menu.
        "chat_down" => {
            if agent.menu_open() {
                agent.menu_step(1);
            }
        }
        "chat_up" => {
            if agent.menu_open() {
                agent.menu_step(-1);
            }
        }
        "back" => {
            if !agent.close_menu() {
                agent.to_list();
            }
            vim.reset();
        }
        "chat" => {
            if agent.screen() == zdt_agentui::Screen::Agent {
                agent.to_chat();
            }
        }

        // The decision surface.
        "approve" => agent.decide(zdt_agent::ask::Decision::Allow),
        "always" => agent.decide(zdt_agent::ask::Decision::AllowAlways),
        "decline" => agent.decide(zdt_agent::ask::Decision::Deny),
        "confirm" => confirm(&agent),
        "implement" => agent.implement(),

        // The menus, and the plan.
        "mode" => {
            agent.to_chat();
            agent.open_menu(zdt_agentui::MenuKind::Mode);
        }
        "model" => {
            agent.to_chat();
            agent.open_menu(zdt_agentui::MenuKind::Model);
        }
        "effort" => {
            agent.to_chat();
            agent.open_menu(zdt_agentui::MenuKind::Effort);
        }
        "plan" => plan(&agent, workspace),

        other => {
            if let Some(number) = other.strip_prefix("choose_") {
                if let Ok(number) = number.parse::<usize>()
                    && number >= 1
                    && !agent.menu_take_at(number - 1)
                {
                    agent.choose(number - 1);
                }
                return;
            }
            workspace.say(format!("agent.{other} is not built yet"));
        }
    }
}

/// `<CR>` in the chat: an open menu first, a several-choice question next, then the plan.
fn confirm(agent: &zdt_agentui::AgentUi) {
    if agent.menu_open() {
        agent.menu_take();
        return;
    }
    if agent.client().asks_untracked().is_empty() {
        if agent.client().plan().is_some() {
            agent.implement();
        }
        return;
    }
    agent.confirm_question();
}

/// Puts the selected thread in plan mode.
fn plan(agent: &zdt_agentui::AgentUi, workspace: &Workspace) {
    let Some(thread) = agent.selected_untracked() else {
        workspace.say("no thread is selected");
        return;
    };
    agent
        .client()
        .set_mode(thread, zdt_agent::mode::RuntimeMode::Plan);
    workspace.say("the thread plans; an empty send takes the plan when it comes");
}

/// Puts back the turn on review, or the last one.
fn revert(agent: &zdt_agentui::AgentUi) {
    match agent.review().and_then(|review| review.turn) {
        Some(turn) => agent.revert_turn(turn),
        None => agent.revert_last(),
    }
}

/// Takes the row under the caret away, asking first when a worktree goes with it.
fn delete(agent: &zdt_agentui::AgentUi) {
    let Some(shell) = agent.shell_at(agent.at()) else {
        return;
    };
    if !shell.worktree {
        agent.delete_at();
        return;
    }
    let deleting = agent.clone();
    agent.host().ask_line(
        "Delete the thread and remove its worktree? y or n",
        "",
        std::rc::Rc::new(move |typed: String| {
            if typed.trim().eq_ignore_ascii_case("y") {
                deleting.delete_at();
            }
        }),
    );
}

/// What the picker calls one provider instance.
fn instance_detail(provider: &str) -> &'static str {
    match provider {
        "claude" => "Claude Code",
        "codex" => "Codex",
        _ => "",
    }
}

/// Makes a thread in the current project, asking which instance drives it when there is a
/// choice.
fn new_thread(agent: &zdt_agentui::AgentUi, workspace: &Workspace) {
    use crate::picker::{Deed, Picker, Row, Source, Target};

    let root = workspace.project().root().to_path_buf();
    let instances = agent.host().instances();
    if instances.len() <= 1 {
        agent.create_in(root, String::new());
        return;
    }
    let Some(picker) = zgui::reactive::use_local_context::<Picker>() else {
        agent.create_in(root, String::new());
        return;
    };
    let rows = instances
        .into_iter()
        .map(|(name, provider)| {
            let (agent, root, chosen) = (agent.clone(), root.clone(), name.clone());
            let mut row = Row::plain(
                name,
                Target::Run(Deed::new(move || {
                    agent.create_in(root.clone(), chosen.clone());
                })),
            )
            .with_detail(instance_detail(&provider));
            if let Some(mark) = zdt_icons::brand(&provider) {
                row = row.with_icon(mark);
            }
            row
        })
        .collect();
    picker.open(Source::Given {
        title: "New thread with",
        rows,
        typed: None,
    });
}

/// A picker over the branches, making a worktree thread off the chosen one.
fn worktree(agent: &zdt_agentui::AgentUi, workspace: &Workspace) {
    use crate::picker::{Deed, Picker, Row, Source, Target};

    let Some(picker) = zgui::reactive::use_local_context::<Picker>() else {
        return;
    };
    let root = workspace.project().root().to_path_buf();
    let from_origin =
        crate::settings::use_settings().with_untracked(|config| config.agent.start_from_origin);
    let instances = agent.host().instances();
    let agent = agent.clone();
    let workspace = workspace.clone();

    zdt_view::detached(async move {
        let read = {
            let root = root.clone();
            zgui::task::blocking(move || {
                let repo = zdt_git::Repo::open(&root).ok()?;
                zdt_git::branches(&repo).ok()
            })
            .await
        };
        let Some(branches) = read else {
            workspace.say("this project is not in a git repository");
            return;
        };

        let mut rows: Vec<Row> = branches
            .into_iter()
            .filter(|branch| !branch.remote)
            .map(|branch| {
                let name = branch.name.clone();
                let (agent, root) = (agent.clone(), root.clone());
                let (picker, instances) = (picker.clone(), instances.clone());
                Row::plain(
                    branch.name.clone(),
                    Target::Run(Deed::new(move || {
                        // The branch is chosen; the instance comes next, when there is a
                        // choice to make.
                        if instances.len() <= 1 {
                            agent.create_worktree_in(
                                root.clone(),
                                name.clone(),
                                from_origin,
                                String::new(),
                            );
                            return;
                        }
                        let rows = instances
                            .iter()
                            .map(|(instance, provider)| {
                                let (agent, root, base) =
                                    (agent.clone(), root.clone(), name.clone());
                                let chosen = instance.clone();
                                let mut row = Row::plain(
                                    instance.clone(),
                                    Target::Run(Deed::new(move || {
                                        agent.create_worktree_in(
                                            root.clone(),
                                            base.clone(),
                                            from_origin,
                                            chosen.clone(),
                                        );
                                    })),
                                )
                                .with_detail(instance_detail(provider));
                                if let Some(mark) = zdt_icons::brand(provider) {
                                    row = row.with_icon(mark);
                                }
                                row
                            })
                            .collect();
                        picker.open(Source::Given {
                            title: "Worktree thread with",
                            rows,
                            typed: None,
                        });
                    })),
                )
                .with_detail(if branch.current { "current" } else { "" })
                .with_glyph(
                    if branch.current {
                        "\u{25cf}"
                    } else {
                        "\u{f062c}"
                    },
                    if branch.current {
                        "zdt-git-added"
                    } else {
                        "zui-color-muted-foreground"
                    },
                )
            })
            .collect();
        // The checked-out branch first: it is the base nine times out of ten.
        rows.sort_by_key(|row| row.detail != "current");

        if rows.is_empty() {
            workspace.say("no branches here; commit something first");
            return;
        }
        picker.open(Source::Given {
            title: "Worktree from branch",
            rows,
            typed: None,
        });
    });
}

/// A picker of snooze spans for the thread under the caret.
fn snooze(agent: &zdt_agentui::AgentUi, workspace: &Workspace) {
    use crate::picker::{Deed, Picker, Row, Source, Target};

    let Some(shell) = agent.caret_shell() else {
        workspace.say("no thread under the caret");
        return;
    };
    let Some(picker) = zgui::reactive::use_local_context::<Picker>() else {
        return;
    };
    const SPANS: &[(&str, u64)] = &[
        ("20 minutes", 20 * 60_000),
        ("1 hour", 3_600_000),
        ("3 hours", 3 * 3_600_000),
        ("8 hours", 8 * 3_600_000),
        ("1 day", 24 * 3_600_000),
        ("3 days", 3 * 24 * 3_600_000),
        ("1 week", 7 * 24 * 3_600_000),
    ];
    let client = agent.client().clone();
    let mut rows: Vec<Row> = SPANS
        .iter()
        .map(|(word, span)| {
            let (client, span) = (client.clone(), *span);
            Row::plain(
                (*word).to_owned(),
                Target::Run(Deed::new(move || {
                    client.snooze(shell.id, zdt_core::state::now_ms() + span);
                })),
            )
        })
        .collect();
    if shell.snoozed_until != 0 {
        let client = client.clone();
        rows.insert(
            0,
            Row::plain(
                "Wake now".to_owned(),
                Target::Run(Deed::new(move || client.snooze(shell.id, 0))),
            ),
        );
    }
    picker.open(Source::Given {
        title: "Snooze for",
        rows,
        typed: None,
    });
}

/// A picker over every thread, over every project.
///
/// The rows carry the title, the project and the branch, so typing filters over all three.
/// Text that matches no row runs a content search through the daemon instead, and the answer
/// comes back as a fresh picker over the threads whose conversations have the words.
fn find(agent: &zdt_agentui::AgentUi) {
    let Some(picker) = zgui::reactive::use_local_context::<crate::picker::Picker>() else {
        return;
    };
    let rows = agent
        .client()
        .threads()
        .into_iter()
        .map(|shell| {
            let opening = agent.clone();
            let thread = shell.id;
            let line = if shell.branch.is_empty() {
                format!("{}  \u{00b7}  {}", shell.title, shell.project)
            } else {
                format!(
                    "{}  \u{00b7}  {}  \u{00b7}  {}",
                    shell.title, shell.project, shell.branch
                )
            };
            crate::picker::source::Row::plain(
                line,
                crate::picker::source::Target::Run(crate::picker::source::Deed::new(move || {
                    opening.open_thread(thread);
                })),
            )
        })
        .collect();
    let searching = agent.clone();
    picker.open(crate::picker::source::Source::Given {
        title: "Agent threads",
        rows,
        typed: Some(crate::picker::source::Typed::new(move |typed: &str| {
            search_content(&searching, typed);
        })),
    });
}

/// Asks the daemon which conversations contain `words`, and opens a picker over the answer.
fn search_content(agent: &zdt_agentui::AgentUi, words: &str) {
    let words = words.trim().to_owned();
    if words.is_empty() {
        return;
    }
    agent.client().search(words.clone());

    // The answer comes over the socket; the effect waits for it, opens the picker, and ends.
    let waiting = agent.clone();
    let landing = zgui::reactive::RenderEffect::new(move |done: Option<bool>| {
        if done == Some(true) {
            return true;
        }
        if !waiting.client().has_found() {
            return false;
        }
        let Some((query, found)) = waiting.client().take_found() else {
            return true;
        };
        if query != words {
            return true;
        }
        let Some(picker) = zgui::reactive::use_local_context::<crate::picker::Picker>() else {
            return true;
        };
        if found.is_empty() {
            waiting.host().say("no conversation has those words");
            return true;
        }
        let rows = found
            .into_iter()
            .map(|row| {
                let opening = waiting.clone();
                crate::picker::source::Row::plain(
                    format!("{}  \u{00b7}  {}", row.title, row.project),
                    crate::picker::source::Target::Run(crate::picker::source::Deed::new(
                        move || opening.open_thread(row.thread),
                    )),
                )
                .with_detail(row.snippet)
            })
            .collect();
        picker.open(crate::picker::source::Source::Given {
            title: "Conversations with the words",
            rows,
            typed: None,
        });
        true
    });
    std::mem::forget(landing);
}
