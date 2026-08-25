//! Newline-delimited JSON off a child's pipe.

use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};

/// Reads `stream` line by line, handing each JSON value to `on_value`.
///
/// A line that is not JSON is logged and skipped: a harness prints warnings onto the same pipe
/// sometimes, and one stray line must not end the session. Returns when the stream ends.
pub async fn each_value(
    stream: impl AsyncRead + Unpin,
    mut on_value: impl FnMut(serde_json::Value),
) {
    let mut lines = BufReader::new(stream).lines();
    loop {
        match lines.next_line().await {
            Ok(Some(line)) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                match serde_json::from_str::<serde_json::Value>(trimmed) {
                    Ok(value) => on_value(value),
                    Err(error) => {
                        tracing::debug!("a line that is not JSON: {error}; line: {trimmed:.120}");
                    }
                }
            }
            Ok(None) => return,
            Err(error) => {
                tracing::warn!("the pipe broke: {error}");
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn values_arrive_one_per_line_and_noise_is_skipped() {
        let text = "{\"a\":1}\nnot json\n\n{\"a\":2}\n";
        let mut seen = Vec::new();
        each_value(text.as_bytes(), |value| seen.push(value["a"].clone())).await;
        assert_eq!(seen, vec![serde_json::json!(1), serde_json::json!(2)]);
    }
}
