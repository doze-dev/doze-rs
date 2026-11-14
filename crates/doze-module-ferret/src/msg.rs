//! OP_MSG framing (the only opcode FerretDB v2 answers besides the legacy hello) and what a reply means: bytes in,
//! bytes out, no I/O, so the fuzz target includes this file by path.

// fuzz:parser fuzz_reply

use crate::bson::{Bson, Doc, Malformed};

/// OP_MSG's opcode.
pub(crate) const OP_MSG: i32 = 2013;
/// The most a reply may be (MongoDB's `maxMessageSizeBytes`).
pub(crate) const MAX_MESSAGE: usize = 48_000_000;
/// Header (16) + flag bits (4) + section kind (1) + the smallest document (5).
pub(crate) const MIN_MESSAGE: usize = 26;

/// How a command failed.
#[derive(Debug)]
pub(crate) enum Failure {
    /// The socket: nothing listening, closed, or no answer in time.
    Io(std::io::Error),
    /// The reply was not a message this client can read.
    Malformed(Malformed),
    /// The server ran the command and refused it.
    Command {
        code: i64,
        name: String,
        message: String,
    },
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Failure::Io(e) => write!(f, "{e}"),
            Failure::Malformed(m) => write!(f, "{m}"),
            Failure::Command {
                code,
                name,
                message,
            } if name.is_empty() => write!(f, "{message} (code {code})"),
            Failure::Command {
                code,
                name,
                message,
            } => write!(f, "{message} ({name}, code {code})"),
        }
    }
}

impl Failure {
    /// The server's error code, when it answered with one.
    pub(crate) fn code(&self) -> Option<i64> {
        match self {
            Failure::Command { code, .. } => Some(*code),
            _ => None,
        }
    }
}

/// The message carrying `body` as OP_MSG section 0, with no flags.
pub(crate) fn frame(request_id: i32, body: &Doc) -> Vec<u8> {
    let doc = body.to_bytes();
    let len = i32::try_from(16 + 4 + 1 + doc.len()).unwrap_or(i32::MAX);
    let mut m = Vec::with_capacity(doc.len() + 21);
    for n in [len, request_id, 0, OP_MSG] {
        m.extend_from_slice(&n.to_le_bytes());
    }
    m.extend_from_slice(&0u32.to_le_bytes());
    m.push(0);
    m.extend_from_slice(&doc);
    m
}

/// The body of a whole OP_MSG reply (header included). Document-sequence sections (kind 1) are stepped over; a
/// checksum, when flagged, is not verified (a local socket corrupts nothing) but is cut off.
pub(crate) fn reply(msg: &[u8], request_id: i32) -> Result<Doc, Malformed> {
    let word = |at: usize| -> Result<i32, Malformed> {
        let b = msg.get(at..at + 4).ok_or(Malformed("message cut short"))?;
        let mut a = [0; 4];
        a.copy_from_slice(b);
        Ok(i32::from_le_bytes(a))
    };
    if usize::try_from(word(0)?).ok() != Some(msg.len()) || msg.len() < MIN_MESSAGE {
        return Err(Malformed("message length does not match"));
    }
    if word(8)? != request_id {
        return Err(Malformed("a reply to another request"));
    }
    if word(12)? != OP_MSG {
        return Err(Malformed("not an OP_MSG reply"));
    }
    let flags = word(16)?;
    let end = if flags & 1 == 1 {
        msg.len() - 4
    } else {
        msg.len()
    };
    let mut rest = msg.get(20..end).ok_or(Malformed("message cut short"))?;
    let mut body = None;
    while let Some((&kind, after)) = rest.split_first() {
        let size = after.get(..4).map(|b| {
            let mut a = [0; 4];
            a.copy_from_slice(b);
            usize::try_from(i32::from_le_bytes(a)).unwrap_or(usize::MAX)
        });
        let size = size
            .filter(|s| *s >= 4 && *s <= after.len())
            .ok_or(Malformed("section length out of range"))?;
        let (section, next) = after.split_at(size);
        match kind {
            0 if body.is_none() => body = Some(Doc::from_bytes(section)?),
            0 => return Err(Malformed("two body sections")),
            1 => {}
            _ => return Err(Malformed("unknown section kind")),
        }
        rest = next;
    }
    body.ok_or(Malformed("no body section"))
}

/// The command's own failure, if the server reported one (`ok` other than 1).
/// A write command (`insert`) says `ok: 1` and lists what it refused in `writeErrors`: the first of those is the
/// failure. A refusal with no words of its own carries the reply, so nothing is lost.
pub(crate) fn outcome(reply: Doc) -> Result<Doc, Failure> {
    let refusal = |d: &Doc| Failure::Command {
        code: d.int("code").unwrap_or(0),
        name: d.str("codeName").unwrap_or_default().to_string(),
        message: d
            .str("errmsg")
            .map_or_else(|| format!("the command failed: {d:?}"), str::to_string),
    };
    // FerretDB answers some commands (dropIndexes) with `ok: true` rather than a number.
    let ok = reply.int("ok") == Some(1) || reply.get("ok") == Some(&Bson::Bool(true));
    if !ok {
        return Err(refusal(&reply));
    }
    let first = reply
        .array("writeErrors")
        .and_then(|e| e.first())
        .or_else(|| reply.get("writeConcernError"));
    match first {
        Some(Bson::Doc(e)) => Err(refusal(e)),
        _ => Ok(reply),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    /// A reply as a server frames it: the header with `response_to`, flags, and the sections given.
    fn server(response_to: i32, flags: u32, sections: &[(u8, Vec<u8>)]) -> Vec<u8> {
        let mut body = flags.to_le_bytes().to_vec();
        for (kind, bytes) in sections {
            body.push(*kind);
            body.extend_from_slice(bytes);
        }
        if flags & 1 == 1 {
            body.extend_from_slice(&[0; 4]);
        }
        let mut m = Vec::new();
        for n in [16 + body.len() as i32, 99, response_to, OP_MSG] {
            m.extend_from_slice(&n.to_le_bytes());
        }
        m.extend(body);
        m
    }

    #[test]
    fn a_request_reads_back_as_its_own_reply() {
        let cmd = Doc::new().with("ping", 1).with("$db", "admin");
        let msg = frame(7, &cmd);
        assert_eq!(msg.len(), 16 + 4 + 1 + cmd.to_bytes().len());
        let ok = Doc::new().with("ok", Bson::Double(1.0));
        assert_eq!(reply(&server(7, 0, &[(0, ok.to_bytes())]), 7).unwrap(), ok);
    }

    #[test]
    fn sequences_and_checksums_are_stepped_over() {
        let ok = Doc::new().with("ok", 1);
        let mut seq = (4 + 2 + 5i32).to_le_bytes().to_vec();
        seq.extend_from_slice(b"d\0");
        seq.extend_from_slice(&Doc::new().to_bytes());
        let m = server(3, 1, &[(1, seq), (0, ok.to_bytes())]);
        assert_eq!(reply(&m, 3).unwrap(), ok);
    }

    #[test]
    fn foreign_and_broken_replies_are_refused() {
        let ok = Doc::new().with("ok", 1).to_bytes();
        assert!(reply(&server(4, 0, &[(0, ok.clone())]), 5).is_err());
        assert!(reply(&server(4, 0, &[(2, ok.clone())]), 4).is_err());
        assert!(reply(&server(4, 0, &[(0, ok.clone()), (0, ok.clone())]), 4).is_err());
        assert!(reply(&server(4, 0, &[]), 4).is_err());
        let mut short = server(4, 0, &[(0, ok)]);
        short.pop();
        assert!(reply(&short, 4).is_err());
    }

    #[test]
    fn a_write_error_is_a_failure_though_the_reply_says_ok() {
        let dup = Doc::new()
            .with("code", 11000)
            .with("errmsg", "E11000 duplicate key");
        let r = Doc::new()
            .with("ok", 1)
            .with("n", 0)
            .with("writeErrors", vec![Bson::Doc(dup)]);
        assert_eq!(outcome(r).unwrap_err().code(), Some(11000));
        assert!(outcome(Doc::new().with("ok", 1).with("n", 2)).is_ok());
        assert!(outcome(Doc::new().with("ok", true)).is_ok());
        assert!(outcome(Doc::new().with("ok", false)).is_err());
    }

    #[test]
    fn a_refusal_carries_the_servers_words() {
        let no = Doc::new()
            .with("ok", Bson::Double(0.0))
            .with("errmsg", "Collection already exists")
            .with("code", 48)
            .with("codeName", "NamespaceExists");
        let e = outcome(no).unwrap_err();
        assert_eq!(e.code(), Some(48));
        assert_eq!(
            e.to_string(),
            "Collection already exists (NamespaceExists, code 48)"
        );
    }
}
