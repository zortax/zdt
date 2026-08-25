//! The agent daemon.
//!
//! One process per user, owning every agent thread: their history in SQLite, their live provider
//! sessions, and the socket the editor speaks to. It outlives the editor on purpose — closing a
//! window must not end a turn that is half-way through a refactor.
//!
//! ```text
//! zdt ──socket──► zdt-agentd ──stdio──► claude
//!                     │
//!                 agent.sqlite
//! ```

mod engine;
mod mock;
mod provider;
mod server;
mod single;
mod store;

use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    if let Some(flag) = args.next() {
        match flag.as_str() {
            "--version" => {
                println!("zdt-agentd {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            other => anyhow::bail!("unknown argument: {other}"),
        }
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("ZDT_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();

    // The one socket, taken before the runtime costs anything. A second daemon finding the first
    // alive says nothing and leaves.
    let Some(listener) = single::claim()? else {
        tracing::info!("a daemon is already running");
        return Ok(());
    };

    let state = state_dir()?;
    std::fs::create_dir_all(&state)?;

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(run(listener, state))
}

/// Where the daemon keeps its database and its logs.
fn state_dir() -> anyhow::Result<PathBuf> {
    zdt_core::state::State::discover()
        .map(|state| state.agent())
        .ok_or_else(|| anyhow::anyhow!("there is no state directory"))
}

async fn run(listener: std::os::unix::net::UnixListener, state: PathBuf) -> anyhow::Result<()> {
    let pool = store::open(&state.join("agent.sqlite")).await?;
    store::reconcile(&pool).await?;

    // What the provider is run as, read once at start from the editor's own configuration.
    let config = zdt_core::config::Paths::discover()
        .and_then(|paths| zdt_core::config::load(&paths.config()).ok())
        .unwrap_or_default();

    let (commands, inbox) = tokio::sync::mpsc::unbounded_channel();
    let (events, adapter_inbox) = tokio::sync::mpsc::unbounded_channel();

    let logs = state.join("logs");
    let providers = provider::Providers::from_config(&config, &events, Some(&logs));

    // Adapter events join the one command queue, so everything that touches state is serial.
    {
        let commands = commands.clone();
        let mut adapter_inbox = adapter_inbox;
        tokio::spawn(async move {
            while let Some(event) = adapter_inbox.recv().await {
                if commands.send(engine::Cmd::Adapter(event)).is_err() {
                    return;
                }
            }
        });
    }

    server::accept(listener, commands.clone())?;
    tracing::info!("zdt-agentd is up");
    let default_mode = zdt_agent::mode::RuntimeMode::named(&config.agent.default_mode);
    let default_mode = if default_mode == zdt_agent::mode::RuntimeMode::Unknown {
        zdt_agent::mode::RuntimeMode::Supervised
    } else {
        default_mode
    };
    // Where worktrees are made: what the configuration says, or under the state directory.
    let worktrees = if config.agent.worktrees.is_empty() {
        state.join("worktrees")
    } else {
        PathBuf::from(&config.agent.worktrees)
    };

    // The housekeeping beat, once a minute for as long as the engine listens.
    {
        let commands = commands.clone();
        tokio::spawn(async move {
            let mut beat = tokio::time::interval(std::time::Duration::from_secs(60));
            beat.tick().await;
            loop {
                beat.tick().await;
                if commands.send(engine::Cmd::Tick).is_err() {
                    return;
                }
            }
        });
    }

    let tuning = engine::Tuning {
        default_mode,
        worktrees,
        auto_settle_days: config.agent.auto_settle_days,
        idle_minutes: config.agent.idle_minutes,
        log_days: config.agent.log_days,
        titles: config.agent.titles,
        title_model: config.agent.title_model.clone(),
        commit_instance: config.agent.commit_instance.clone(),
        commit_model: config.agent.commit_model.clone(),
        logs,
    };
    engine::Engine::new(pool, providers, tuning, commands.clone())
        .run(inbox)
        .await;
    Ok(())
}
