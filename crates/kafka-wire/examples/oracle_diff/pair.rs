//! A connection to each broker, and a step sent to both and compared.

use std::fmt::Debug;
use std::io::{Read, Write};
use std::net::TcpStream;

use bytes::Bytes;
use kafka_protocol::messages::{
    ApiKey, DescribeClientQuotasResponse, ProduceResponse, ResponseHeader,
};
use kafka_protocol::protocol::{Decodable, Encodable};
use kafka_wire::testing::{framed, request_payload};

use crate::requests::{describe, norm_describe, produce};

pub(crate) struct Conn {
    pub(crate) stream: TcpStream,
    pub(crate) next: i32,
}

impl Conn {
    pub(crate) fn open(addr: &str) -> Self {
        Self {
            stream: TcpStream::connect(addr).unwrap_or_else(|e| panic!("connect {addr}: {e}")),
            next: 1,
        }
    }

    pub(crate) fn call<Q: Encodable, A: Decodable>(
        &mut self,
        key: ApiKey,
        version: i16,
        req: &Q,
    ) -> A {
        self.next += 1;
        let payload = request_payload(key, version, self.next, Some("oracle-diff"), req);
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

pub(crate) struct Pair {
    pub(crate) kafka: Conn,
    pub(crate) doze: Conn,
    pub(crate) differences: usize,
    pub(crate) steps: usize,
}

impl Pair {
    /// Sends `req` to both, normalises both answers with `norm`, and compares them.
    pub(crate) fn same<Q: Encodable, A: Decodable + Debug>(
        &mut self,
        name: &str,
        key: ApiKey,
        version: i16,
        req: &Q,
        norm: impl Fn(&mut A),
    ) -> (A, A) {
        let mut k: A = self.kafka.call(key, version, req);
        let mut d: A = self.doze.call(key, version, req);
        norm(&mut k);
        norm(&mut d);
        let (ks, ds) = (format!("{k:#?}"), format!("{d:#?}"));
        self.steps += 1;
        if ks == ds {
            println!("same  {name} ({key:?} v{version})");
        } else {
            self.differences += 1;
            println!("DIFF  {name} ({key:?} v{version})\n--- kafka\n{ks}\n--- doze\n{ds}");
        }
        (k, d)
    }

    /// New connections to both: a throttled connection is muted, and the next step should not wait for it.
    pub(crate) fn reconnect(&mut self, kafka: &str, doze: &str) {
        self.kafka = Conn::open(kafka);
        self.doze = Conn::open(doze);
    }

    /// Produces one record to both, again until each takes it (a partition Kafka has just made may not have a leader yet).
    pub(crate) fn produce(&mut self, name: &str, partition: i32, value: &str) {
        let req = produce(name, partition, value);
        for conn in [&mut self.kafka, &mut self.doze] {
            for attempt in 0.. {
                let r: ProduceResponse = conn.call(ApiKey::Produce, 9, &req);
                let ok = r
                    .responses
                    .iter()
                    .flat_map(|t| &t.partition_responses)
                    .all(|p| p.error_code == 0);
                if ok {
                    break;
                }
                assert!(attempt < 100, "{name}-{partition} never took a record");
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
    }

    /// Waits until Kafka's answer to a DescribeClientQuotas of everything is doze-kafka's: Kafka applies a quota change in its
    /// controller and answers DescribeClientQuotas from the broker's copy of the metadata, which follows a moment later.
    pub(crate) fn settle_quotas(&mut self) {
        let everything = describe(&[], false);
        for _ in 0..50 {
            let (mut k, mut d): (DescribeClientQuotasResponse, DescribeClientQuotasResponse) =
                self.both(ApiKey::DescribeClientQuotas, 1, &everything);
            norm_describe(&mut k);
            norm_describe(&mut d);
            if format!("{k:?}") == format!("{d:?}") {
                // The controller's own quota managers are told by another listener of the same log.
                std::thread::sleep(std::time::Duration::from_millis(300));
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        println!("note  Kafka's quotas never came to doze-kafka's");
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
