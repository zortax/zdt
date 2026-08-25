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
        Launch::Agent(verb) => agent(verb),
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
            let host = zdt::session::host::SessionHost::new(global.clone(), root);
            let keeping = starting.path().map(std::path::Path::to_path_buf);
            // The session the first window opens on. Made here rather than in that window, so a
            // window that is suspended and rebuilt attaches to the same one.
            host.open(starting);
            if let Some(listener) = listener.borrow_mut().take() {
                host.serve(listener);
            }
            // The agent surface: the daemon connection and the state every window shares.
            zdt::agent::install(&host, global.settings());
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

    // The daemon outlives the window unless the configuration says it goes with it. Threads
    // keep working either way until the shutdown lands; the next window reattaches.
    let stops = zdt_core::config::Paths::discover()
        .and_then(|paths| zdt_core::config::load(&paths.config()).ok())
        .is_some_and(|config| config.agent.stop_on_exit);
    if stops {
        tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .build()?
            .block_on(zdt_agent_client::stop_running_daemon());
    }

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

/// `zdt agent <verb>`: one question to the agent daemon, printed and done.
fn agent(verb: zdt::cli::AgentVerb) -> anyhow::Result<()> {
    use zdt::cli::AgentVerb;
    use zdt_agent::protocol::{ClientMsg, ServerMsg};

    let directory = zdt_ipc::client::directory()
        .ok_or_else(|| anyhow::anyhow!("there is no runtime directory for the daemon's socket"))?;
    let socket = directory.join("agentd.sock");

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async move {
        let Ok(stream) = tokio::net::UnixStream::connect(&socket).await else {
            anyhow::bail!("no agent daemon is running");
        };
        let (mut reading, mut writing) = stream.into_split();
        let hello = ClientMsg::Hello {
            version: zdt_agent::VERSION,
            pid: std::process::id(),
        };
        zdt_agent::wire::write(&mut writing, &hello).await?;
        let pid = match zdt_agent::wire::read::<ServerMsg>(&mut reading).await? {
            ServerMsg::Welcome { pid, .. } => pid,
            ServerMsg::Refused { reason } => anyhow::bail!("the daemon refused: {reason}"),
            other => anyhow::bail!("the daemon said something unexpected: {other:?}"),
        };
        match verb {
            AgentVerb::Status => {
                println!(
                    "zdt-agentd is running, pid {pid}, protocol {}",
                    zdt_agent::VERSION
                );
            }
            AgentVerb::Stop => {
                zdt_agent::wire::write(&mut writing, &ClientMsg::Shutdown).await?;
                println!("asked zdt-agentd (pid {pid}) to stop");
            }
            AgentVerb::List => loop {
                // The thread list is pushed right after the welcome; everything else is skipped.
                let ServerMsg::Shells { threads } =
                    zdt_agent::wire::read::<ServerMsg>(&mut reading).await?
                else {
                    continue;
                };
                if threads.is_empty() {
                    println!("no threads");
                }
                for shell in threads {
                    println!(
                        "{:>4}  {:<10} {:<28} {}",
                        shell.id.0,
                        shell.state.word(),
                        clip(&shell.title, 28),
                        shell.project,
                    );
                }
                break;
            },
        }
        Ok(())
    })
}

/// At most `most` characters, with an ellipsis when something was cut.
fn clip(text: &str, most: usize) -> String {
    if text.chars().count() <= most {
        return text.to_owned();
    }
    let mut cut: String = text.chars().take(most.saturating_sub(1)).collect();
    cut.push('\u{2026}');
    cut
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
