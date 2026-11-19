//! `InvokeWithResponseStream`'s events, framed as an AWS event stream (`aws_wire::eventstream`).

use aws_wire::eventstream::event;

/// A piece of the function's response.
pub(crate) fn chunk(payload: &[u8]) -> Vec<u8> {
    event("PayloadChunk", "application/octet-stream", payload)
}

/// The end of the stream, with the error the function ended in, if any, and the tail of its log.
pub(crate) fn complete(error: Option<(&str, &str)>, log: Option<&str>) -> Vec<u8> {
    let mut body = serde_json::Map::new();
    if let Some((code, details)) = error {
        body.insert("ErrorCode".into(), code.into());
        body.insert("ErrorDetails".into(), details.into());
    }
    if let Some(l) = log {
        body.insert("LogResult".into(), l.into());
    }
    event(
        "InvokeComplete",
        "application/json",
        serde_json::Value::Object(body).to_string().as_bytes(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_events_carry_their_names() {
        assert!(chunk(b"hello").windows(12).any(|w| w == b"PayloadChunk"));
        assert!(
            complete(Some(("Unhandled", "boom")), None)
                .windows(9)
                .any(|w| w == b"ErrorCode")
        );
    }
}
