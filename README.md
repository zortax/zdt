# zdt

zdt is a desktop code editor written in Rust. It provides a Vim-style modal
editing model on top of the `zgui` UI and editor libraries.

The editor currently includes:

- buffers, split windows, a file tree, fuzzy pickers, and integrated terminals;
- configurable modal keymaps, motions, operators, text objects, macros, and
  Ex commands;
- language-server diagnostics, completion, navigation, hover, rename, and
  formatting;
- Git status, diff, staging, commit, branch, log, and blame views;
- coding agents (Claude Code, Codex) in a thread inbox with worktrees,
  per-turn checkpoints, and diff review;
- themes, sessions, and configuration that reloads while the editor is running.

## Build and run

The workspace uses Rust 2024 edition. After configuring the local dependencies:

```sh
cargo build -p zdt
cargo run -p zdt -- [PATH ...]
```

A directory argument selects the project root. File arguments are opened at
startup; when no directory is given, zdt discovers the project from the first
file or the current directory.

Set `ZDT_LOG` to a `tracing` filter when additional diagnostics are needed:

```sh
ZDT_LOG=zdt=debug cargo run -p zdt -- .
```

## Configuration

Configuration is read from the platform configuration directory under `zdt`,
or from `ZDT_CONFIG_DIR` when set. On Linux, the default is typically
`~/.config/zdt`.

The main files are:

- `config.toml` for editor settings and language servers;
- `keymap.toml` and `keymap-tree.toml` for key overrides;
- `user.css` for style overrides;
- `themes/` for custom light and dark themes.

Missing files use built-in defaults. Configuration, keymaps, CSS, and themes
are watched for changes.

## Agents

zdt drives coding agents through a separate daemon, `zdt-agentd`. The daemon
owns every agent thread and outlives the editor, so a turn keeps running when
the window closes. The editor starts the daemon on demand; build both binaries
and keep them side by side:

```sh
cargo build --release -p zdt -p zdt-agentd
```

`<Leader>a` opens the agent key group: `aa` toggles the agent view, `ae` the
sidebar, `an` starts a thread in the current project, `aw` starts one in a git
worktree of its own, `af` finds a thread (typed text that matches no title
searches the conversations), `am`/`aM` choose the mode and the model, `ad`
reviews the changes, `ar` reverts the last turn, and `ac`/`aC` open the commit
modal (the second pushes too). The modal lists the changed files with their
counts, drafts a message, a description, and a branch name for review — the
model never commits — and commits on `<C-CR>`, or onto a fresh branch on
`<C-b>`. The same commit button sits in the editor chrome while a thread is
selected. `commit_instance`/`commit_model` choose what drafts; codex is
preferred when nothing is set. The
composer's chips choose the model, the mode, and the reasoning effort the
provider offers, and a ring beside the send button shows how much of the
context window the conversation has used.

Inside the sidebar, threads are triaged with single keys: `p` pins (`<C-k>` and
`<C-j>` reorder), `s` settles, `z` snoozes with presets, `a` archives, `A`
shows the archived shelf, `u` marks unread, `r` renames, and `R` has a name
generated. A thread names itself after its first turn, and a worktree thread's
temporary branch takes the generated name too. The composer keeps an unsent
draft per thread, stored in the daemon.

Work an agent starts beside itself — background subagents, dynamic workflows —
shows as a strip above the composer, and the thread counts as working for as
long as any of it runs, however idle the main agent is. A workflow row opens a
modal with the run's phases, the agents inside them, and the script's log,
following the run live.

The `[agent]` section of `config.toml` configures the surface: provider
instances under `[agent.instances.<name>]` (a `provider` word, a `binary`, a
`home` directory — one home is one account — and a default `model`),
`auto_settle_days`, `idle_minutes`, `log_days`, `titles`, and `title_model`.
The daemon outlives the window: threads keep working while no editor is open,
and the next window reattaches to whatever is running. `stop_on_exit = true`
stops the daemon — running turns included — when the window closes.
The `mock` provider word gives an instance that streams synthetic turns for
layout and load work without an agent behind it.

From the command line, `zdt agent list` prints every thread, `zdt agent
status` says whether the daemon runs, and `zdt agent stop` stops it.

## Development checks

```sh
cargo check -p zdt
cargo test --workspace
cargo clippy --workspace --all-targets
```
