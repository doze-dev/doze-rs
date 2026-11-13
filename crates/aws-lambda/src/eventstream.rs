//! The binary framing of an AWS event stream (`application/vnd.amazon.eventstream`), which carries `InvokeWithResponseStream`'s answer:
//! each message is a prelude (total length, headers length, CRC of both), string headers, a payload, and a CRC of all of it.

/// One event message with the string headers `:message-type`, `:event-type` and `:content-type`.
pub(crate) fn event(event_type: &str, content_type: &str, payload: &[u8]) -> Vec<u8> {
    let mut headers = Vec::new();
    for (name, value) in [
        (":event-type", event_type),
        (":content-type", content_type),
        (":message-type", "event"),
    ] {
        headers.push(u8::try_from(name.len()).unwrap_or(u8::MAX));
        headers.extend_from_slice(name.as_bytes());
        headers.push(7);
        headers.extend_from_slice(&u16::try_from(value.len()).unwrap_or(u16::MAX).to_be_bytes());
        headers.extend_from_slice(value.as_bytes());
    }
    let total = u32::try_from(16 + headers.len() + payload.len()).unwrap_or(u32::MAX);
    let mut out = Vec::with_capacity(total as usize);
    out.extend_from_slice(&total.to_be_bytes());
    out.extend_from_slice(
        &u32::try_from(headers.len())
            .unwrap_or(u32::MAX)
            .to_be_bytes(),
    );
    let prelude = crc32fast::hash(&out);
    out.extend_from_slice(&prelude.to_be_bytes());
    out.extend_from_slice(&headers);
    out.extend_from_slice(payload);
    let all = crc32fast::hash(&out);
    out.extend_from_slice(&all.to_be_bytes());
    out
}

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

    fn be32(b: &[u8], at: usize) -> u32 {
        b.get(at..at + 4)
            .map_or(0, |s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
    }

    #[test]
    fn a_message_is_framed_and_checksummed() {
        let m = chunk(b"hello");
        assert_eq!(be32(&m, 0) as usize, m.len());
        assert_eq!(be32(&m, 8), crc32fast::hash(m.get(..8).unwrap_or_default()));
        assert_eq!(
            be32(&m, m.len() - 4),
            crc32fast::hash(m.get(..m.len() - 4).unwrap_or_default())
        );
        assert!(m.windows(5).any(|w| w == b"hello"));
        assert!(m.windows(12).any(|w| w == b"PayloadChunk"));
        assert!(
            complete(Some(("Unhandled", "boom")), None)
                .windows(9)
                .any(|w| w == b"ErrorCode")
        );
    }
}
