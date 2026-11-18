//! The PostgreSQL wire protocol, as much as the filter reads: the client's first packets (encryption requests, a
//! cancel request, the startup message) and the server's messages until it is ready for queries. Pure parsing over
//! bytes, fuzzed (`fuzz/fuzz_targets/fuzz_wire.rs`); the I/O is in `filter`.

/// The codes a startup-phase packet carries in place of a protocol version.
pub(crate) const SSL_REQUEST: u32 = 80_877_103;
pub(crate) const GSSENC_REQUEST: u32 = 80_877_104;
pub(crate) const CANCEL_REQUEST: u32 = 80_877_102;
/// Protocol 3.x: the major in the top 16 bits.
const PROTOCOL_3: u32 = 3 << 16;

/// The longest startup packet read: PostgreSQL's own limit (Go allowed 1 MiB, held for every slow client).
pub(crate) const MAX_STARTUP: usize = 10_000;
/// The longest server message read during the handshake.
pub(crate) const MAX_MESSAGE: usize = 1 << 20;
/// A cancel key's secret: 4 bytes in protocol 3.0, up to 256 in 3.2 (PostgreSQL 18).
const MAX_SECRET: usize = 256;

/// A cancel key: the backend's process id and its secret.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct Key {
    pub pid: u32,
    pub secret: Vec<u8>,
}

/// What a client's first packet asks for, read from its 8-byte header (length, code).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum First {
    /// SSLRequest (`true`) or GSSENCRequest (`false`): answered with one byte.
    Encrypt(bool),
    /// A CancelRequest: the rest (`len - 8` bytes) is the key.
    Cancel(usize),
    /// A StartupMessage: the rest (`len - 8` bytes) is its body.
    Startup(usize),
}

// fuzz:parser fuzz_wire
/// The client's packet header: its length (which counts itself) and code.
pub(crate) fn first(header: [u8; 8]) -> Result<First, String> {
    let [a, b, c, d, e, f, g, h] = header;
    let len = u32::from_be_bytes([a, b, c, d]) as usize;
    let code = u32::from_be_bytes([e, f, g, h]);
    match code {
        SSL_REQUEST | GSSENC_REQUEST if len == 8 => Ok(First::Encrypt(code == SSL_REQUEST)),
        SSL_REQUEST | GSSENC_REQUEST => Err(format!("malformed negotiation packet (len={len})")),
        CANCEL_REQUEST if (12..=12 + MAX_SECRET).contains(&len) => Ok(First::Cancel(len - 8)),
        CANCEL_REQUEST => Err(format!("malformed cancel request (len={len})")),
        _ if (8..=MAX_STARTUP).contains(&len) => Ok(First::Startup(len - 8)),
        // A TLS record (ClientHello) where a startup belongs: PostgreSQL 17's direct SSL.
        _ if a == 0x16 => {
            Err("direct SSL is not supported; connect with sslnegotiation=postgres".into())
        }
        _ => Err(format!("implausible startup length {len}")),
    }
}

/// A CancelRequest's key, from the bytes after its header.
pub(crate) fn cancel_key(rest: &[u8]) -> Option<Key> {
    let (pid, secret) = rest.split_first_chunk::<4>()?;
    (!secret.is_empty()).then(|| Key {
        pid: u32::from_be_bytes(*pid),
        secret: secret.to_vec(),
    })
}

/// A CancelRequest for `key`.
pub(crate) fn cancel_request(key: &Key) -> Vec<u8> {
    let len = u32::try_from(12 + key.secret.len()).unwrap_or(u32::MAX);
    let mut b = Vec::with_capacity(12 + key.secret.len());
    b.extend(len.to_be_bytes());
    b.extend(CANCEL_REQUEST.to_be_bytes());
    b.extend(key.pid.to_be_bytes());
    b.extend(&key.secret);
    b
}

/// The database a startup message asks for (its user's name when it names none), checked as Go did: a startup with
/// neither is refused before any server is woken for it.
pub(crate) fn startup_database(header: [u8; 8], body: &[u8]) -> Result<String, String> {
    let [.., e, f, g, h] = header;
    let code = u32::from_be_bytes([e, f, g, h]);
    let (mut database, mut user) = (String::new(), String::new());
    if code >> 16 == PROTOCOL_3 >> 16 {
        let mut parts = body.split(|b| *b == 0).take_while(|p| !p.is_empty());
        while let (Some(k), Some(v)) = (parts.next(), parts.next()) {
            match k {
                b"database" => database = String::from_utf8_lossy(v).into_owned(),
                b"user" => user = String::from_utf8_lossy(v).into_owned(),
                _ => {}
            }
        }
    }
    let db = if database.is_empty() { user } else { database };
    if db.is_empty() {
        return Err("startup message specified no database".into());
    }
    Ok(db)
}

/// A server message's header: its type and the length of its payload.
pub(crate) fn message(header: [u8; 5]) -> Result<(u8, usize), String> {
    let [kind, a, b, c, d] = header;
    let len = u32::from_be_bytes([a, b, c, d]) as usize;
    if !(4..=MAX_MESSAGE + 4).contains(&len) {
        return Err(format!("implausible message length {len}"));
    }
    Ok((kind, len - 4))
}

/// BackendKeyData's payload as a key, if it is one.
pub(crate) fn backend_key(kind: u8, payload: &[u8]) -> Option<Key> {
    if kind != b'K' || payload.len() > 4 + MAX_SECRET {
        return None;
    }
    cancel_key(payload)
}

/// BackendKeyData's payload for `key`.
pub(crate) fn key_payload(key: &Key) -> Vec<u8> {
    let mut b = key.pid.to_be_bytes().to_vec();
    b.extend(&key.secret);
    b
}

/// A FATAL ErrorResponse, so a client shows a clean error.
pub(crate) fn error_response(code: &str, message: &str) -> Vec<u8> {
    let mut p = Vec::new();
    for (t, v) in [
        (b'S', "FATAL"),
        (b'V', "FATAL"),
        (b'C', code),
        (b'M', message),
    ] {
        p.push(t);
        p.extend(v.bytes().filter(|b| *b != 0));
        p.push(0);
    }
    p.push(0);
    let len = u32::try_from(p.len() + 4).unwrap_or(u32::MAX);
    let mut m = vec![b'E'];
    m.extend(len.to_be_bytes());
    m.extend(p);
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(len: u32, code: u32) -> [u8; 8] {
        let mut h = [0u8; 8];
        h[..4].copy_from_slice(&len.to_be_bytes());
        h[4..].copy_from_slice(&code.to_be_bytes());
        h
    }

    #[test]
    fn first_packets() {
        assert_eq!(first(header(8, SSL_REQUEST)), Ok(First::Encrypt(true)));
        assert_eq!(first(header(8, GSSENC_REQUEST)), Ok(First::Encrypt(false)));
        assert!(first(header(9, SSL_REQUEST)).is_err());
        assert_eq!(first(header(16, CANCEL_REQUEST)), Ok(First::Cancel(8)));
        assert_eq!(first(header(44, CANCEL_REQUEST)), Ok(First::Cancel(36)));
        assert!(first(header(8, CANCEL_REQUEST)).is_err());
        assert_eq!(first(header(20, PROTOCOL_3)), Ok(First::Startup(12)));
        assert!(first(header(4, PROTOCOL_3)).is_err());
        assert!(first(header(u32::MAX, PROTOCOL_3)).is_err());
    }

    #[test]
    fn startup_names_its_database_or_its_user() {
        let h = header(0, PROTOCOL_3 | 2);
        assert_eq!(
            startup_database(h, b"user\0app\0database\0shop\0\0"),
            Ok("shop".into())
        );
        assert_eq!(startup_database(h, b"user\0app\0\0"), Ok("app".into()));
        assert!(startup_database(h, b"\0").is_err());
    }

    #[test]
    fn keys_round_trip_whatever_their_length() {
        let k = Key {
            pid: 7,
            secret: vec![1; 32],
        };
        let req = cancel_request(&k);
        assert_eq!(
            first(req[..8].try_into().unwrap_or([0; 8])),
            Ok(First::Cancel(36))
        );
        assert_eq!(cancel_key(&req[8..]), Some(k.clone()));
        assert_eq!(backend_key(b'K', &key_payload(&k)), Some(k));
        assert_eq!(backend_key(b'Z', &[0; 8]), None);
        assert_eq!(message([b'Z', 0, 0, 0, 5]), Ok((b'Z', 1)));
        assert!(message([b'Z', 0, 0, 0, 3]).is_err());
        let e = error_response("28000", "no");
        assert_eq!(e[0], b'E');
        assert_eq!(
            u32::from_be_bytes([e[1], e[2], e[3], e[4]]) as usize,
            e.len() - 1
        );
    }
}
