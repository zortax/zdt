//! One message on the wire.
//!
//! A four-byte little-endian length, then that many bytes of JSON. Length-prefixed because a
//! stream has no message boundaries of its own, and little-endian because every machine this runs
//! on already is.

use std::io::{Read, Write};

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::IpcError;

/// The largest frame either end will read.
///
/// Far above anything this protocol sends, and low enough that a stream of nonsense cannot ask
/// for a gigabyte of memory.
pub const LIMIT: u32 = 1 << 20;

/// Writes one message.
///
/// # Errors
///
/// When the value will not encode, or the stream will not take it.
pub fn write<T: Serialize>(out: &mut impl Write, value: &T) -> Result<(), IpcError> {
    let bytes =
        serde_json::to_vec(value).map_err(|error| IpcError::Malformed(error.to_string()))?;
    let length = u32::try_from(bytes.len()).map_err(|_| IpcError::TooLarge(u32::MAX))?;
    if length > LIMIT {
        return Err(IpcError::TooLarge(length));
    }
    out.write_all(&length.to_le_bytes())?;
    out.write_all(&bytes)?;
    out.flush()?;
    Ok(())
}

/// Reads one message.
///
/// # Errors
///
/// When the stream ends, when the frame is longer than [`LIMIT`], or when the bytes are not the
/// message that was expected.
pub fn read<T: DeserializeOwned>(input: &mut impl Read) -> Result<T, IpcError> {
    let mut header = [0u8; 4];
    input.read_exact(&mut header)?;
    let length = u32::from_le_bytes(header);
    if length > LIMIT {
        return Err(IpcError::TooLarge(length));
    }
    let mut bytes = vec![0u8; length as usize];
    input.read_exact(&mut bytes)?;
    serde_json::from_slice(&bytes).map_err(|error| IpcError::Malformed(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Request, Response};

    #[test]
    fn a_message_written_is_the_message_read() {
        let mut buffer = Vec::new();
        write(
            &mut buffer,
            &Request::Hello {
                version: crate::VERSION,
                pid: 42,
            },
        )
        .expect("it writes");

        let back: Request = read(&mut buffer.as_slice()).expect("it reads");
        let Request::Hello { pid, .. } = back else {
            panic!("a hello");
        };
        assert_eq!(pid, 42);
    }

    #[test]
    fn two_messages_in_one_stream_stay_apart() {
        // The whole reason there is a length in front of each.
        let mut buffer = Vec::new();
        write(&mut buffer, &Response::Pong).expect("it writes");
        write(
            &mut buffer,
            &Response::Refused {
                reason: "no".to_owned(),
            },
        )
        .expect("it writes");

        let mut stream = buffer.as_slice();
        assert!(matches!(
            read::<Response>(&mut stream).expect("it reads"),
            Response::Pong
        ));
        assert!(matches!(
            read::<Response>(&mut stream).expect("it reads"),
            Response::Refused { .. }
        ));
    }

    #[test]
    fn a_frame_larger_than_the_limit_is_refused_before_it_is_read() {
        // A stream of nonsense must not be able to ask for a gigabyte.
        let mut bytes = (LIMIT + 1).to_le_bytes().to_vec();
        bytes.extend_from_slice(b"whatever");
        let error = read::<Request>(&mut bytes.as_slice()).expect_err("it refuses");
        assert!(matches!(error, IpcError::TooLarge(_)));
    }

    #[test]
    fn a_stream_that_ends_half_way_is_an_error_and_not_a_guess() {
        let bytes = [4u8, 0, 0, 0, b'{'];
        let error = read::<Request>(&mut bytes.as_slice()).expect_err("it refuses");
        assert!(matches!(error, IpcError::Io(_)));
    }

    #[test]
    fn bytes_that_are_not_a_message_are_reported_rather_than_guessed_at() {
        let mut buffer = Vec::new();
        let text = b"not json at all";
        buffer.extend_from_slice(&(text.len() as u32).to_le_bytes());
        buffer.extend_from_slice(text);
        let error = read::<Request>(&mut buffer.as_slice()).expect_err("it refuses");
        assert!(matches!(error, IpcError::Malformed(_)));
    }
}
