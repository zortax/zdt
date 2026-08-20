//! The entry point.
//!
//! Three things happen here, in order. The command line is read. A zdt that is already running is
//! given the chance to take the work, in which case this process says one sentence and exits.
//! Only then does anything cost a window.

use std::cell::RefCell;
use std::path::PathBuf;

use zgui::prelude::*;
use zgui::view;

use zdt::app::RootProps;
use zdt::cli::{Launch, Open};
use zdt::session::SessionKey;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("ZDT_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn,zdt=info")),
        )
        .with_writer(std::io::stderr)
        .init();

    match zdt::cli::parse() {
        Launch::Help => {
            print!("{}", zdt::cli::USAGE);
            Ok(())
        }
        Launch::Version => {
            println!("zdt {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Launch::List => list(),
        Launch::Kill(dir) => kill(&dir),
        Launch::Open(open) => start(open),
    }
}

/// Works in a directory, either by handing it to a running zdt or by becoming the one that runs.
fn start(open: Open) -> anyhow::Result<()> {
    use zdt_ipc::client::HandOff;

    let listener = if open.standalone {
        None
    } else {
        let request = zdt_ipc::Request::Attach {
            dir: open.key.path().map(PathBuf::from).unwrap_or_default(),
            files: open.files.clone(),
            new_window: open.new_window,
        };
        match zdt_ipc::client::hand_off(&request) {
            // A zdt is already working here. It has the directory now.
            HandOff::Delivered(answer) => {
                if let zdt_ipc::Response::Refused { reason } = answer {
                    anyhow::bail!(reason);
                }
                return Ok(());
            }
            HandOff::Host(listener) => Some(listener),
            // No socket to be had. One editor, talking to nobody, which is far better than not
            // starting at all.
            HandOff::Alone => None,
        }
    };

    run(open, listener)
}

/// Opens the window, and everything under it.
fn run(open: Open, listener: Option<std::os::unix::net::UnixListener>) -> anyhow::Result<()> {
    let Open { key, files, .. } = open;

    // Before the first window and before any background work: the runtime has to be the one
    // behind `background` and `blocking` from the start, and it is what lets a language server's
    // socket be awaited on the interface thread.
    let _tokio = zgui::tokio::install()?;

    // The scope every session hangs off, taken here because here is the one place with nothing
    // above it. A session made under a window would lose its buffers when that window closed.
    zgui::reactive::install()?;
    let root = zdt::session::host::detached_root();

    let title = zdt::app::window::title_for(&key.name());
    let starting = key.clone();
    // Taken once by the setup below. A `RefCell` because a `UnixListener` cannot be copied and
    // the setup closure is built before it runs.
    let listener = RefCell::new(listener);

    app()
        .with_application_id("dev.zdt.Editor")
        .with_title(title)
        .with_size(1280.0, 800.0)
        .with_min_size(480.0, 320.0)
        // The two that make the window this application's to draw. `assets/css/frame.css` draws
        // what the desktop dropped.
        .with_decorations(Decorations::None)
        .with_transparent(true)
        .with_stylesheet(zdt::assets::sheet())
        // Everything shared by every window and every session, in the one scope above them all.
        .with_context(move || {
            let global = zdt::app::global::install();
            let host = zdt::session::host::SessionHost::new(global, root);
            let keeping = starting.path().map(std::path::Path::to_path_buf);
            // The session the first window opens on. Made here rather than in that window, so a
            // window that is suspended and rebuilt attaches to the same one.
            host.open(starting);
            if let Some(listener) = listener.borrow_mut().take() {
                host.serve(listener);
            }
            zdt::session::host::provide(host);
            // Sessions for directories that are gone, and ones nobody has opened in months.
            if let Some(keeping) = keeping {
                zdt::session::save::prune_soon(keeping);
            }
        })
        .run(move || {
            let host = zdt::session::host::use_host();
            let session = host
                .find(&key)
                .and_then(|id| host.session(id))
                .expect("the session opened above every window is still there");
            view! { Root(session = session, files = files.clone()) }
        })?;

    Ok(())
}

/// `--list`: what a running zdt has open.
fn list() -> anyhow::Result<()> {
    match ask(zdt_ipc::Request::List)? {
        zdt_ipc::Response::Sessions { sessions } if sessions.is_empty() => {
            println!("no sessions");
        }
        zdt_ipc::Response::Sessions { sessions } => {
            for session in sessions {
                // A star on the ones a window is looking at, which is what tmux marks too.
                let mark = if session.attached { "*" } else { " " };
                println!(
                    "{mark} {:<24} {:>3} buffers  {}",
                    session.name,
                    session.buffers,
                    session.dir.display(),
                );
            }
        }
        other => anyhow::bail!("the running zdt said something unexpected: {other:?}"),
    }
    Ok(())
}

/// `--kill <DIR>`: takes a running zdt's session away.
fn kill(dir: &std::path::Path) -> anyhow::Result<()> {
    let dir = SessionKey::of(dir)
        .and_then(|key| key.path().map(PathBuf::from))
        .unwrap_or_else(|| dir.to_path_buf());
    match ask(zdt_ipc::Request::Kill { dir })? {
        zdt_ipc::Response::Killed { dir } => {
            println!("killed {}", dir.display());
            Ok(())
        }
        other => anyhow::bail!("the running zdt said something unexpected: {other:?}"),
    }
}

/// Asks a running zdt one question, or says there is none.
fn ask(request: zdt_ipc::Request) -> anyhow::Result<zdt_ipc::Response> {
    use zdt_ipc::client::HandOff;
    match zdt_ipc::client::hand_off(&request) {
        HandOff::Delivered(answer) => Ok(answer),
        // Becoming the host is not what was asked for: the socket is given straight back, so the
        // next real `zdt` can bind it.
        HandOff::Host(listener) => {
            drop(listener);
            if let Some(directory) = zdt_ipc::client::directory() {
                let _ = std::fs::remove_file(zdt_ipc::client::socket_in(&directory));
            }
            anyhow::bail!("no zdt is running")
        }
        HandOff::Alone => anyhow::bail!("no zdt is running"),
    }
}
