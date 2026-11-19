//! The binary framing of an AWS event stream (`application/vnd.amazon.eventstream`), which carries the answers of streaming operations
//! (Lambda's `InvokeWithResponseStream`, CloudWatch Logs' `StartLiveTail`): each message is a prelude (total length, headers length,
//! CRC of both), string headers, a payload, and a CRC of all of it.

/// The content type of an event-stream answer.
pub const CONTENT_TYPE: &str = "application/vnd.amazon.eventstream";

/// One message with string headers (header value type 7) and `payload`.
#[must_use]
pub fn message(headers: &[(&str, &str)], payload: &[u8]) -> Vec<u8> {
    let mut head = Vec::new();
    for (name, value) in headers {
        head.push(u8::try_from(name.len()).unwrap_or(u8::MAX));
        head.extend_from_slice(name.as_bytes());
        head.push(7);
        head.extend_from_slice(&u16::try_from(value.len()).unwrap_or(u16::MAX).to_be_bytes());
        head.extend_from_slice(value.as_bytes());
    }
    let total = u32::try_from(16 + head.len() + payload.len()).unwrap_or(u32::MAX);
    let mut out = Vec::with_capacity(total as usize);
    out.extend_from_slice(&total.to_be_bytes());
    out.extend_from_slice(&u32::try_from(head.len()).unwrap_or(u32::MAX).to_be_bytes());
    let prelude = crc32fast::hash(&out);
    out.extend_from_slice(&prelude.to_be_bytes());
    out.extend_from_slice(&head);
    out.extend_from_slice(payload);
    let all = crc32fast::hash(&out);
    out.extend_from_slice(&all.to_be_bytes());
    out
}

/// An event message: `:event-type`, `:content-type` and `:message-type: event`.
#[must_use]
pub fn event(event_type: &str, content_type: &str, payload: &[u8]) -> Vec<u8> {
    message(
        &[
            (":event-type", event_type),
            (":content-type", content_type),
            (":message-type", "event"),
        ],
        payload,
    )
}

/// A modelled exception sent in the stream (`:message-type: exception`), its payload the error's JSON members.
#[must_use]
pub fn exception(exception_type: &str, payload: &[u8]) -> Vec<u8> {
    message(
        &[
            (":exception-type", exception_type),
            (":content-type", "application/json"),
            (":message-type", "exception"),
        ],
        payload,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn be32(b: &[u8], at: usize) -> u32 {
        b.get(at..at + 4)
            .and_then(|s| <[u8; 4]>::try_from(s).ok())
            .map_or(0, u32::from_be_bytes)
    }

    #[test]
    fn a_message_is_framed_and_checksummed() {
        let m = event("PayloadChunk", "application/octet-stream", b"hello");
        assert_eq!(be32(&m, 0) as usize, m.len());
        assert_eq!(be32(&m, 8), crc32fast::hash(m.get(..8).unwrap_or_default()));
        assert_eq!(
            be32(&m, m.len() - 4),
            crc32fast::hash(m.get(..m.len() - 4).unwrap_or_default())
        );
        assert!(m.windows(5).any(|w| w == b"hello"));
        assert!(m.windows(12).any(|w| w == b"PayloadChunk"));
        let x = exception("SessionTimeoutException", b"{}");
        assert!(x.windows(9).any(|w| w == b"exception"));
    }
}
