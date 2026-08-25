//! One message on the wire, asynchronously.
//!
//! The same shape as `zdt-ipc`'s frame — a four-byte little-endian length, then JSON — carried
//! over tokio streams, because both ends of this socket live inside a runtime. The limit is
//! higher: a conversation snapshot carries whole messages, and a megabyte is a long transcript
//! but not an absurd one.

use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::WireError;

/// The largest frame either end will read.
pub const LIMIT: u32 = 16 * 1024 * 1024;

/// Writes one message.
///
/// # Errors
///
/// When the value will not encode, or the stream will not take it.
pub async fn write<T: Serialize>(
    out: &mut (impl AsyncWrite + Unpin),
    value: &T,
) -> Result<(), WireError> {
    let bytes =
        serde_json::to_vec(value).map_err(|error| WireError::Malformed(error.to_string()))?;
    let length = u32::try_from(bytes.len()).map_err(|_| WireError::TooLarge(u32::MAX))?;
    if length > LIMIT {
        return Err(WireError::TooLarge(length));
    }
    out.write_all(&length.to_le_bytes()).await?;
    out.write_all(&bytes).await?;
    out.flush().await?;
    Ok(())
}

/// Reads one message.
///
/// # Errors
///
/// When the stream ends, when the frame is longer than [`LIMIT`], or when the bytes are not the
/// message that was expected.
pub async fn read<T: DeserializeOwned>(
    input: &mut (impl AsyncRead + Unpin),
) -> Result<T, WireError> {
    let mut header = [0u8; 4];
    input.read_exact(&mut header).await?;
    let length = u32::from_le_bytes(header);
    if length > LIMIT {
        return Err(WireError::TooLarge(length));
    }
    let mut bytes = vec![0u8; length as usize];
    input.read_exact(&mut bytes).await?;
    serde_json::from_slice(&bytes).map_err(|error| WireError::Malformed(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{ClientMsg, ServerMsg};

    #[tokio::test]
    async fn a_message_written_is_the_message_read() {
        let mut buffer = Vec::new();
        write(
            &mut buffer,
            &ClientMsg::Hello {
                version: crate::VERSION,
                pid: 42,
            },
        )
        .await
        .expect("it writes");

        let back: ClientMsg = read(&mut buffer.as_slice()).await.expect("it reads");
        assert!(matches!(back, ClientMsg::Hello { pid: 42, .. }));
    }

    #[tokio::test]
    async fn two_messages_in_one_stream_stay_apart() {
        let mut buffer = Vec::new();
        write(&mut buffer, &ServerMsg::Welcome { version: 1, pid: 7 })
            .await
            .expect("it writes");
        write(
            &mut buffer,
            &ServerMsg::Refused {
                reason: "no".to_owned(),
            },
        )
        .await
        .expect("it writes");

        let mut stream = buffer.as_slice();
        assert!(matches!(
            read::<ServerMsg>(&mut stream).await.expect("it reads"),
            ServerMsg::Welcome { .. }
        ));
        assert!(matches!(
            read::<ServerMsg>(&mut stream).await.expect("it reads"),
            ServerMsg::Refused { .. }
        ));
    }

    #[tokio::test]
    async fn a_frame_larger_than_the_limit_is_refused_before_it_is_read() {
        let mut bytes = (LIMIT + 1).to_le_bytes().to_vec();
        bytes.extend_from_slice(b"whatever");
        let error = read::<ClientMsg>(&mut bytes.as_slice())
            .await
            .expect_err("it refuses");
        assert!(matches!(error, WireError::TooLarge(_)));
    }
}
