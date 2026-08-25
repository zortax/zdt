//! The socket, and the clients on it.
//!
//! Each connection is two tasks: one reads frames and hands them to the engine, one drains the
//! engine's answers back onto the wire. All state lives in the engine; a connection is nothing
//! but pipes and a number.

use std::sync::atomic::{AtomicU64, Ordering};

use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use zdt_agent::protocol::{ClientMsg, ServerMsg};
use zdt_agent::{VERSION, wire};

use crate::engine::Cmd;

/// Names one connection for the engine's client map.
pub type ClientId = u64;

/// Starts accepting clients on `listener`.
pub fn accept(
    listener: std::os::unix::net::UnixListener,
    commands: UnboundedSender<Cmd>,
) -> anyhow::Result<()> {
    listener.set_nonblocking(true)?;
    let listener = tokio::net::UnixListener::from_std(listener)?;
    static NEXT: AtomicU64 = AtomicU64::new(1);

    tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let id = NEXT.fetch_add(1, Ordering::Relaxed);
                    let commands = commands.clone();
                    tokio::spawn(async move { converse(stream, id, commands).await });
                }
                Err(error) => {
                    tracing::warn!("a client would not connect: {error}");
                    return;
                }
            }
        }
    });
    Ok(())
}

/// One client, from hello to hangup.
async fn converse(stream: tokio::net::UnixStream, id: ClientId, commands: UnboundedSender<Cmd>) {
    let (mut reading, mut writing) = stream.into_split();

    // The handshake, before the engine hears anything.
    match wire::read::<ClientMsg>(&mut reading).await {
        Ok(ClientMsg::Hello { version, .. }) if version == VERSION => {
            let welcome = ServerMsg::Welcome {
                version: VERSION,
                pid: std::process::id(),
            };
            if wire::write(&mut writing, &welcome).await.is_err() {
                return;
            }
        }
        Ok(ClientMsg::Hello { version, .. }) => {
            let _ = wire::write(
                &mut writing,
                &ServerMsg::Refused {
                    reason: format!("this daemon speaks version {VERSION}, not {version}"),
                },
            )
            .await;
            return;
        }
        Ok(_) => {
            let _ = wire::write(
                &mut writing,
                &ServerMsg::Refused {
                    reason: "expected a hello".to_owned(),
                },
            )
            .await;
            return;
        }
        Err(_) => return,
    }

    // The writer: everything the engine says to this client, in order.
    let (to_client, mut outbox) = unbounded_channel::<ServerMsg>();
    let writer = tokio::spawn(async move {
        while let Some(message) = outbox.recv().await {
            if wire::write(&mut writing, &message).await.is_err() {
                return;
            }
        }
    });

    if commands.send(Cmd::Connected { id, to_client }).is_err() {
        return;
    }

    // The reader. A frame that does not read as a message is skipped rather than fatal, so a
    // client one release ahead can still speak the parts both sides know.
    while let Ok(value) = wire::read::<serde_json::Value>(&mut reading).await {
        match serde_json::from_value::<ClientMsg>(value) {
            Ok(message) => {
                if commands.send(Cmd::Client { id, message }).is_err() {
                    break;
                }
            }
            Err(error) => tracing::debug!("a message this daemon has no word for: {error}"),
        }
    }
    let _ = commands.send(Cmd::Disconnected { id });
    writer.abort();
}
