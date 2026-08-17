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

## Development checks

```sh
cargo check -p zdt
cargo test --workspace
cargo clippy --workspace --all-targets
```
