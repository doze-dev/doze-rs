//! A connection to a broker that may log in with SASL/PLAIN, and a step sent to both brokers and compared.

use std::fmt::Debug;
use std::io::{Read, Write};
use std::net::TcpStream;

use bytes::Bytes;
use kafka_protocol::messages::{
    ApiKey, ResponseHeader, SaslAuthenticateRequest, SaslAuthenticateResponse,
    SaslHandshakeRequest, SaslHandshakeResponse,
};
use kafka_protocol::protocol::{Decodable, Encodable, StrBytes};
use kafka_wire::testing::{framed, request_payload};

pub(crate) struct Conn {
    stream: TcpStream,
    next: i32,
}

impl Conn {
    pub(crate) fn open(addr: &str) -> Self {
        Self {
            stream: TcpStream::connect(addr).unwrap_or_else(|e| panic!("connect {addr}: {e}")),
            next: 1,
        }
    }

    /// A connection logged in as `user` with SASL/PLAIN.
    pub(crate) fn login(addr: &str, user: &str, password: &str) -> Self {
        let mut c = Self::open(addr);
        let mut h = SaslHandshakeRequest::default();
        h.mechanism = StrBytes::from_static_str("PLAIN");
        let r: SaslHandshakeResponse = c.call(ApiKey::SaslHandshake, 1, &h);
        assert_eq!(r.error_code, 0, "{addr}: handshake");
        let mut a = SaslAuthenticateRequest::default();
        a.auth_bytes = Bytes::from(format!("\0{user}\0{password}").into_bytes());
        let r: SaslAuthenticateResponse = c.call(ApiKey::SaslAuthenticate, 2, &a);
        assert_eq!(r.error_code, 0, "{addr}: login as {user}");
        c
    }

    pub(crate) fn call<Q: Encodable, A: Decodable>(
        &mut self,
        key: ApiKey,
        version: i16,
        req: &Q,
    ) -> A {
        self.next += 1;
        let payload = request_payload(key, version, self.next, Some("oracle-admin"), req);
        self.stream.write_all(&framed(&payload)).unwrap();
        let mut len = [0u8; 4];
        self.stream.read_exact(&mut len).unwrap();
        let mut body = vec![0u8; usize::try_from(u32::from_be_bytes(len)).unwrap()];
        self.stream.read_exact(&mut body).unwrap();
        let mut buf = Bytes::from(body);
        let head = ResponseHeader::decode(&mut buf, key.response_header_version(version)).unwrap();
        assert_eq!(head.correlation_id, self.next);
        let out = A::decode(&mut buf, version).unwrap();
        assert!(buf.is_empty(), "{key:?} v{version}: bytes left over");
        out
    }
}

/// One connection to each broker, as the same user.
pub(crate) struct Pair {
    pub(crate) kafka: Conn,
    pub(crate) doze: Conn,
}

/// The tally of a run.
#[derive(Default)]
pub(crate) struct Tally {
    pub(crate) steps: usize,
    pub(crate) differences: usize,
    pub(crate) known: usize,
}

impl Pair {
    /// Sends `req` to both, normalises both answers with `norm`, and compares them.
    pub(crate) fn same<Q: Encodable, A: Decodable + Debug + Clone>(
        &mut self,
        t: &mut Tally,
        name: &str,
        key: ApiKey,
        version: i16,
        req: &Q,
        norm: impl Fn(&mut A),
    ) -> (A, A) {
        self.same2(t, name, key, version, (req, req), norm, None)
    }

    /// As [`Pair::same`] with a request of each broker's own (one that names its log directory, or its token), and, if
    /// `known` says why, a difference that is written down (counted, not failed).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn same2<Q: Encodable, A: Decodable + Debug + Clone>(
        &mut self,
        t: &mut Tally,
        name: &str,
        key: ApiKey,
        version: i16,
        (to_kafka, to_doze): (&Q, &Q),
        norm: impl Fn(&mut A),
        known: Option<&str>,
    ) -> (A, A) {
        let mut k: A = self.kafka.call(key, version, to_kafka);
        let mut d: A = self.doze.call(key, version, to_doze);
        let raw = (k.clone(), d.clone());
        norm(&mut k);
        norm(&mut d);
        let (ks, ds) = (format!("{k:#?}"), format!("{d:#?}"));
        t.steps += 1;
        if ks == ds {
            if known.is_some() {
                println!(
                    "STALE {name} ({key:?} v{version}): written down as different, and the same"
                );
                t.differences += 1;
            } else {
                println!("same  {name} ({key:?} v{version})");
            }
        } else if let Some(why) = known {
            t.known += 1;
            println!("known {name} ({key:?} v{version}): {why}\n--- kafka\n{ks}\n--- doze\n{ds}");
        } else {
            t.differences += 1;
            println!("DIFF  {name} ({key:?} v{version})\n--- kafka\n{ks}\n--- doze\n{ds}");
        }
        raw
    }

    /// Sends `req` to both without comparing (setup).
    pub(crate) fn both<Q: Encodable, A: Decodable>(
        &mut self,
        key: ApiKey,
        version: i16,
        req: &Q,
    ) -> (A, A) {
        (
            self.kafka.call(key, version, req),
            self.doze.call(key, version, req),
        )
    }
}
