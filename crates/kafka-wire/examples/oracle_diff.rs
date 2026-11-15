//! The same requests to real Apache Kafka and to doze-kafka, their answers compared field by field: the oracle for the APIs
//! no recording covers (OffsetForLeaderEpoch, DescribeLogDirs, DescribeClientQuotas, AlterClientQuotas and the controller
//! mutation quota of CreateTopics).
//!
//! ```sh
//! cargo run -p kafka-wire --example oracle_diff -- <kafka host:port> <doze host:port>
//! ```
//!
//! Both brokers should be fresh (the run makes topics named `oracle-*` and sets quotas, and removes the quotas at the end);
//! `tools/kafka-oracle/start.sh` starts the real one.
//! What must differ is normalised before the comparison: the log directory's path and the volume's sizes, throttle times
//! (compared as zero or not), and the order of entries Kafka keeps in hash maps. Topics other than `oracle-*` (Kafka's
//! internal ones) are left out of DescribeLogDirs. Exits 1 if any step differs.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_lines,
    reason = "a command-line oracle: a step that cannot be made is a loud failure"
)]

use std::fmt::Debug;
use std::io::{Read, Write};
use std::net::TcpStream;

use bytes::Bytes;
use kafka_protocol::messages::alter_client_quotas_request::{
    EntityData as AlterEntity, EntryData as AlterEntry, OpData,
};
use kafka_protocol::messages::create_topics_request::CreatableTopic;
use kafka_protocol::messages::delete_records_request::{
    DeleteRecordsPartition, DeleteRecordsTopic,
};
use kafka_protocol::messages::describe_client_quotas_request::ComponentData;
use kafka_protocol::messages::describe_log_dirs_request::DescribableLogDirTopic;
use kafka_protocol::messages::offset_for_leader_epoch_request::{
    OffsetForLeaderPartition, OffsetForLeaderTopic,
};
use kafka_protocol::messages::produce_request::{PartitionProduceData, TopicProduceData};
use kafka_protocol::messages::*;
use kafka_protocol::protocol::{Decodable, Encodable, StrBytes};
use kafka_wire::batch::{OneRecord, single_record};
use kafka_wire::testing::{framed, request_payload};

struct Conn {
    stream: TcpStream,
    next: i32,
}

impl Conn {
    fn open(addr: &str) -> Self {
        Self {
            stream: TcpStream::connect(addr).unwrap_or_else(|e| panic!("connect {addr}: {e}")),
            next: 1,
        }
    }

    fn call<Q: Encodable, A: Decodable>(&mut self, key: ApiKey, version: i16, req: &Q) -> A {
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

struct Pair {
    kafka: Conn,
    doze: Conn,
    differences: usize,
    steps: usize,
}

impl Pair {
    /// Sends `req` to both, normalises both answers with `norm`, and compares them.
    fn same<Q: Encodable, A: Decodable + Debug>(
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
    fn reconnect(&mut self, kafka: &str, doze: &str) {
        self.kafka = Conn::open(kafka);
        self.doze = Conn::open(doze);
    }

    /// Produces one record to both, again until each takes it (a partition Kafka has just made may not have a leader yet).
    fn produce(&mut self, name: &str, partition: i32, value: &str) {
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
    fn settle_quotas(&mut self) {
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
    fn both<Q: Encodable, A: Decodable>(&mut self, key: ApiKey, version: i16, req: &Q) -> (A, A) {
        (
            self.kafka.call(key, version, req),
            self.doze.call(key, version, req),
        )
    }
}

fn s(text: &str) -> StrBytes {
    StrBytes::from_string(text.to_owned())
}

fn topic(name: &str) -> TopicName {
    TopicName(s(name))
}

fn create(names: &[(&str, i32)]) -> CreateTopicsRequest {
    let mut req = CreateTopicsRequest::default();
    req.timeout_ms = 30_000;
    for (name, partitions) in names {
        let mut t = CreatableTopic::default();
        t.name = topic(name);
        t.num_partitions = *partitions;
        t.replication_factor = 1;
        req.topics.push(t);
    }
    req
}

fn produce(name: &str, partition: i32, value: &str) -> ProduceRequest {
    let batch = single_record(&OneRecord {
        timestamp_ms: 1_700_000_000_000,
        key: Some(b"k"),
        value: Some(value.as_bytes()),
        headers: &[],
    });
    let mut p = PartitionProduceData::default();
    p.index = partition;
    p.records = Some(batch);
    let mut t = TopicProduceData::default();
    t.name = topic(name);
    t.partition_data.push(p);
    let mut req = ProduceRequest::default();
    req.acks = -1;
    req.timeout_ms = 30_000;
    req.topic_data.push(t);
    req
}

fn epochs(asks: &[(&str, &[Ask])], replica: i32) -> OffsetForLeaderEpochRequest {
    let mut req = OffsetForLeaderEpochRequest::default();
    req.replica_id = BrokerId(replica);
    for (name, parts) in asks {
        let mut t = OffsetForLeaderTopic::default();
        t.topic = topic(name);
        for &(partition, current, epoch) in *parts {
            let mut p = OffsetForLeaderPartition::default();
            p.partition = partition;
            p.current_leader_epoch = current;
            p.leader_epoch = epoch;
            t.partitions.push(p);
        }
        req.topics.push(t);
    }
    req
}

fn log_dirs(asks: Option<&[(&str, &[i32])]>) -> DescribeLogDirsRequest {
    let mut req = DescribeLogDirsRequest::default();
    req.topics = asks.map(|asks| {
        asks.iter()
            .map(|(name, parts)| {
                let mut t = DescribableLogDirTopic::default();
                t.topic = topic(name);
                t.partitions = parts.to_vec();
                t
            })
            .collect()
    });
    req
}

/// The path and the volume differ by nature; topics other than this run's are Kafka's own.
fn norm_log_dirs(r: &mut DescribeLogDirsResponse) {
    for dir in &mut r.results {
        dir.log_dir = s("<log dir>");
        dir.total_bytes = dir.total_bytes.signum();
        dir.usable_bytes = dir.usable_bytes.signum();
        dir.topics.retain(|t| t.name.0.starts_with("oracle-"));
        dir.topics.sort_by(|a, b| a.name.0.cmp(&b.name.0));
        for t in &mut dir.topics {
            t.partitions.sort_by_key(|p| p.partition_index);
        }
    }
}

type Entity<'a> = &'a [(&'a str, Option<&'a str>)];
/// One quota key set to a value or removed (`None`).
type Op<'a> = (&'a str, Option<f64>);
/// One entity's alteration.
type Change<'a> = (Entity<'a>, &'a [Op<'a>]);
/// (partition, current leader epoch, epoch asked about)
type Ask = (i32, i32, i32);
/// A DescribeClientQuotas filter: its name, its components (type, match type, name) and whether it is strict.
type FilterCase<'a> = (&'a str, &'a [(&'a str, i8, Option<&'a str>)], bool);

fn alter(entries: &[Change<'_>], validate: bool) -> AlterClientQuotasRequest {
    let mut req = AlterClientQuotasRequest::default();
    req.validate_only = validate;
    for (entity, ops) in entries {
        let mut e = AlterEntry::default();
        e.entity = entity
            .iter()
            .map(|(t, n)| {
                let mut part = AlterEntity::default();
                part.entity_type = s(t);
                part.entity_name = n.map(s);
                part
            })
            .collect();
        e.ops = ops
            .iter()
            .map(|(key, value)| {
                let mut op = OpData::default();
                op.key = s(key);
                op.value = value.unwrap_or(0.0);
                op.remove = value.is_none();
                op
            })
            .collect();
        req.entries.push(e);
    }
    req
}

/// Kafka answers from hash maps: the order of entries, of an entity's parts and of the values is not a contract.
fn norm_alter(r: &mut AlterClientQuotasResponse) {
    for e in &mut r.entries {
        e.entity.sort_by(|a, b| a.entity_type.cmp(&b.entity_type));
    }
    r.entries.sort_by_key(|e| format!("{:?}", e.entity));
}

fn norm_describe(r: &mut DescribeClientQuotasResponse) {
    if let Some(entries) = &mut r.entries {
        for e in entries.iter_mut() {
            e.entity.sort_by(|a, b| a.entity_type.cmp(&b.entity_type));
            e.values.sort_by(|a, b| a.key.cmp(&b.key));
        }
        entries.sort_by_key(|e| format!("{:?}", e.entity));
    }
}

fn describe(components: &[(&str, i8, Option<&str>)], strict: bool) -> DescribeClientQuotasRequest {
    let mut req = DescribeClientQuotasRequest::default();
    req.strict = strict;
    req.components = components
        .iter()
        .map(|(t, m, name)| {
            let mut c = ComponentData::default();
            c.entity_type = s(t);
            c.match_type = *m;
            c._match = name.map(s);
            c
        })
        .collect();
    req
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [kafka, doze] = args.as_slice() else {
        eprintln!("usage: oracle_diff <kafka host:port> <doze host:port>");
        std::process::exit(2);
    };
    let mut pair = Pair {
        kafka: Conn::open(kafka),
        doze: Conn::open(doze),
        differences: 0,
        steps: 0,
    };
    leader_epochs(&mut pair);
    describe_log_dirs(&mut pair);
    client_quotas(&mut pair);
    mutation_quota(&mut pair, kafka, doze);
    println!("{} steps, {} differences", pair.steps, pair.differences);
    std::process::exit(i32::from(pair.differences > 0));
}

fn leader_epochs(pair: &mut Pair) {
    let _: (CreateTopicsResponse, CreateTopicsResponse) =
        pair.both(ApiKey::CreateTopics, 7, &create(&[("oracle-epochs", 2)]));
    for value in ["a", "bb", "ccc", "dddd"] {
        pair.produce("oracle-epochs", 0, value);
    }
    let cases: &[Ask] = &[
        (0, -1, 0),
        (1, -1, 0),
        (0, -1, 1),
        (0, -1, -1),
        (0, -1, -2),
        (1, -1, -7),
        (0, 0, 0),
        (0, 1, 0),
        (0, -2, 0),
        (0, -9, 3),
        (7, -1, 0),
    ];
    for version in 2..=4 {
        let req = epochs(
            &[("oracle-epochs", cases), ("oracle-nothing", &[(0, -1, 0)])],
            -1,
        );
        pair.same::<_, OffsetForLeaderEpochResponse>(
            "epochs",
            ApiKey::OffsetForLeaderEpoch,
            version,
            &req,
            |_| {},
        );
    }
    // The follower form (a replica id) is answered the same.
    let req = epochs(&[("oracle-epochs", &[(0, 0, 0)])], 1);
    pair.same::<_, OffsetForLeaderEpochResponse>(
        "follower form",
        ApiKey::OffsetForLeaderEpoch,
        4,
        &req,
        |_| {},
    );
    // After the start moves: the epoch begins at the new start.
    let mut del = DeleteRecordsRequest::default();
    del.timeout_ms = 30_000;
    let mut t = DeleteRecordsTopic::default();
    t.name = topic("oracle-epochs");
    let mut p = DeleteRecordsPartition::default();
    p.partition_index = 0;
    p.offset = 2;
    t.partitions.push(p);
    del.topics.push(t);
    let _: (DeleteRecordsResponse, DeleteRecordsResponse) =
        pair.both(ApiKey::DeleteRecords, 2, &del);
    let req = epochs(
        &[("oracle-epochs", &[(0, -1, 0), (0, -1, -2), (0, 0, -5)])],
        -1,
    );
    pair.same::<_, OffsetForLeaderEpochResponse>(
        "after delete-records",
        ApiKey::OffsetForLeaderEpoch,
        4,
        &req,
        |_| {},
    );
}

fn describe_log_dirs(pair: &mut Pair) {
    let _: (CreateTopicsResponse, CreateTopicsResponse) =
        pair.both(ApiKey::CreateTopics, 7, &create(&[("oracle-dirs", 3)]));
    for value in ["one", "two", "three"] {
        pair.produce("oracle-dirs", 1, value);
    }
    for version in 1..=4 {
        pair.same::<_, DescribeLogDirsResponse>(
            "every partition",
            ApiKey::DescribeLogDirs,
            version,
            &log_dirs(None),
            norm_log_dirs,
        );
        let some: &[(&str, &[i32])] = &[
            ("oracle-dirs", &[1, 2, 9]),
            ("oracle-epochs", &[1]),
            ("oracle-nothing", &[0]),
        ];
        pair.same::<_, DescribeLogDirsResponse>(
            "some partitions",
            ApiKey::DescribeLogDirs,
            version,
            &log_dirs(Some(some)),
            norm_log_dirs,
        );
        pair.same::<_, DescribeLogDirsResponse>(
            "none asked",
            ApiKey::DescribeLogDirs,
            version,
            &log_dirs(Some(&[])),
            norm_log_dirs,
        );
    }
}

fn client_quotas(pair: &mut Pair) {
    let alice: Entity<'_> = &[("user", Some("alice"))];
    let alice_c1: Entity<'_> = &[("user", Some("alice")), ("client-id", Some("c1"))];
    let default_user: Entity<'_> = &[("user", None)];
    let default_user_c1: Entity<'_> = &[("user", None), ("client-id", Some("c1"))];
    let default_client: Entity<'_> = &[("client-id", None)];
    let ip: Entity<'_> = &[("ip", Some("127.0.0.1"))];
    let set: &[Change<'_>] = &[
        (
            alice,
            &[
                ("producer_byte_rate", Some(1024.0)),
                ("consumer_byte_rate", Some(2048.0)),
            ],
        ),
        (alice_c1, &[("request_percentage", Some(12.5))]),
        (default_user, &[("controller_mutation_rate", Some(3.0))]),
        (default_user_c1, &[("producer_byte_rate", Some(4096.0))]),
        (default_client, &[("consumer_byte_rate", Some(100.0))]),
        (ip, &[("connection_creation_rate", Some(50.0))]),
    ];
    for version in 0..=1 {
        pair.same::<_, AlterClientQuotasResponse>(
            "validate only",
            ApiKey::AlterClientQuotas,
            version,
            &alter(set, true),
            norm_alter,
        );
    }
    pair.same::<_, DescribeClientQuotasResponse>(
        "nothing set yet",
        ApiKey::DescribeClientQuotas,
        1,
        &describe(&[], false),
        norm_describe,
    );
    pair.same::<_, AlterClientQuotasResponse>(
        "set",
        ApiKey::AlterClientQuotas,
        1,
        &alter(set, false),
        norm_alter,
    );
    pair.settle_quotas();
    let refused: &[Change<'_>] = &[
        (&[], &[("producer_byte_rate", Some(1.0))]),
        (
            &[("group", Some("g"))],
            &[("producer_byte_rate", Some(1.0))],
        ),
        (&[("user", Some(""))], &[("producer_byte_rate", Some(1.0))]),
        (
            &[("ip", Some("127.0.0.1")), ("user", Some("x"))],
            &[("connection_creation_rate", Some(1.0))],
        ),
        (
            &[("ip", Some("no-such-host.invalid"))],
            &[("connection_creation_rate", Some(1.0))],
        ),
        (&[("user", Some("b1"))], &[("bogus", Some(1.0))]),
        (
            &[("user", Some("b2"))],
            &[("producer_byte_rate", Some(0.0))],
        ),
        (
            &[("user", Some("b3"))],
            &[("producer_byte_rate", Some(-5.0))],
        ),
        (
            &[("user", Some("b4"))],
            &[("producer_byte_rate", Some(1.5))],
        ),
        (
            &[("user", Some("b5"))],
            &[("consumer_byte_rate", Some(1e19))],
        ),
        (
            &[("user", Some("b6"))],
            &[("producer_byte_rate", Some(f64::INFINITY))],
        ),
        (
            &[("user", Some("b7"))],
            &[
                ("producer_byte_rate", Some(1.0)),
                ("producer_byte_rate", Some(2.0)),
            ],
        ),
        (&[("ip", None)], &[("connection_creation_rate", Some(3e9))]),
        (&[("ip", None)], &[("producer_byte_rate", Some(1.0))]),
        (
            &[("client-id", Some("c9"))],
            &[("connection_creation_rate", Some(1.0))],
        ),
        (
            &[("user", Some("b8"))],
            &[("producer_byte_rate", Some(10.0))],
        ),
        (
            &[("user", Some("b8"))],
            &[("consumer_byte_rate", Some(10.0))],
        ),
        (&[("user", Some("b9"))], &[("producer_byte_rate", None)]),
    ];
    for version in 0..=1 {
        pair.same::<_, AlterClientQuotasResponse>(
            "refusals",
            ApiKey::AlterClientQuotas,
            version,
            &alter(refused, true),
            norm_alter,
        );
    }
    pair.same::<_, AlterClientQuotasResponse>(
        "refusals, for real",
        ApiKey::AlterClientQuotas,
        1,
        &alter(refused, false),
        norm_alter,
    );
    pair.settle_quotas();
    let filters: &[FilterCase<'_>] = &[
        ("everything", &[], false),
        ("strict nothing", &[], true),
        ("alice", &[("user", 0, Some("alice"))], false),
        ("alice strict", &[("user", 0, Some("alice"))], true),
        ("default user", &[("user", 1, None)], false),
        ("default user strict", &[("user", 1, None)], true),
        ("any user", &[("user", 2, None)], false),
        (
            "any user and client",
            &[("user", 2, None), ("client-id", 2, None)],
            true,
        ),
        ("client c1", &[("client-id", 0, Some("c1"))], false),
        ("default client", &[("client-id", 1, None)], true),
        ("ips", &[("ip", 2, None)], false),
        ("an ip", &[("ip", 0, Some("127.0.0.1"))], true),
        ("nobody", &[("user", 0, Some("nobody"))], false),
        ("empty type", &[("", 0, Some("x"))], false),
        ("twice", &[("user", 2, None), ("user", 0, Some("a"))], false),
        ("unknown type", &[("group", 0, Some("x"))], false),
        ("exact without a name", &[("user", 0, None)], false),
        ("default with a name", &[("user", 1, Some("a"))], false),
        ("specified with a name", &[("user", 2, Some("a"))], false),
        ("unknown match", &[("user", 9, None)], false),
        ("ip and user", &[("ip", 2, None), ("user", 2, None)], false),
    ];
    for (name, components, strict) in filters {
        for version in 0..=1 {
            pair.same::<_, DescribeClientQuotasResponse>(
                name,
                ApiKey::DescribeClientQuotas,
                version,
                &describe(components, *strict),
                norm_describe,
            );
        }
    }
    // Everything removed again: removing the last key of an entity removes the entity.
    let clear: Vec<(Entity<'_>, Vec<Op<'_>>)> = set
        .iter()
        .map(|(e, ops)| (*e, ops.iter().map(|(k, _)| (*k, None)).collect()))
        .collect();
    let clear: Vec<Change<'_>> = clear.iter().map(|(e, ops)| (*e, ops.as_slice())).collect();
    pair.same::<_, AlterClientQuotasResponse>(
        "clear",
        ApiKey::AlterClientQuotas,
        1,
        &alter(&clear, false),
        norm_alter,
    );
    let b8: &[Change<'_>] = &[(
        &[("user", Some("b8"))],
        &[("producer_byte_rate", None), ("consumer_byte_rate", None)],
    )];
    pair.same::<_, AlterClientQuotasResponse>(
        "clear b8",
        ApiKey::AlterClientQuotas,
        1,
        &alter(b8, false),
        norm_alter,
    );
    pair.settle_quotas();
    pair.same::<_, DescribeClientQuotasResponse>(
        "all gone",
        ApiKey::DescribeClientQuotas,
        1,
        &describe(&[], false),
        norm_describe,
    );
}

/// `controller_mutation_rate`: a bucket of 11 seconds' worth; the topic that empties it is made, the next refused.
fn mutation_quota(pair: &mut Pair, kafka: &str, doze: &str) {
    let rate: &[Change<'_>] = &[(
        &[("client-id", Some("oracle-diff"))],
        &[("controller_mutation_rate", Some(1.0))],
    )];
    let _: (AlterClientQuotasResponse, AlterClientQuotasResponse) =
        pair.both(ApiKey::AlterClientQuotas, 1, &alter(rate, false));
    pair.settle_quotas();
    let topics = [
        ("oracle-m1", 5),
        ("oracle-m2", 5),
        ("oracle-m3", 5),
        ("oracle-m4", 5),
    ];
    let norm = |r: &mut CreateTopicsResponse| {
        r.throttle_time_ms = r.throttle_time_ms.signum();
        for t in &mut r.topics {
            t.topic_id = kafka_wire::Uuid::nil();
            t.configs = None;
        }
    };
    pair.same::<_, CreateTopicsResponse>(
        "mutations: strict",
        ApiKey::CreateTopics,
        7,
        &create(&topics),
        norm,
    );
    // Each next step on new connections: the ones above are muted for the throttle time, which would refill the bucket.
    let delete = |name: &str| {
        let mut req = DeleteTopicsRequest::default();
        req.timeout_ms = 30_000;
        req.topic_names.push(topic(name));
        req
    };
    let norm_delete = |r: &mut DeleteTopicsResponse| {
        r.throttle_time_ms = r.throttle_time_ms.signum();
        for t in &mut r.responses {
            t.topic_id = kafka_wire::Uuid::nil();
        }
    };
    pair.reconnect(kafka, doze);
    pair.same::<_, DeleteTopicsResponse>(
        "mutations: strict delete",
        ApiKey::DeleteTopics,
        5,
        &delete("oracle-m1"),
        norm_delete,
    );
    pair.reconnect(kafka, doze);
    pair.same::<_, DeleteTopicsResponse>(
        "mutations: permissive delete",
        ApiKey::DeleteTopics,
        4,
        &delete("oracle-m1"),
        norm_delete,
    );
    let grow = |name: &str, count: i32| {
        let mut req = CreatePartitionsRequest::default();
        req.timeout_ms = 30_000;
        let mut t =
            kafka_protocol::messages::create_partitions_request::CreatePartitionsTopic::default();
        t.name = topic(name);
        t.count = count;
        req.topics.push(t);
        req
    };
    let norm_grow =
        |r: &mut CreatePartitionsResponse| r.throttle_time_ms = r.throttle_time_ms.signum();
    pair.reconnect(kafka, doze);
    pair.same::<_, CreatePartitionsResponse>(
        "mutations: strict grow",
        ApiKey::CreatePartitions,
        3,
        &grow("oracle-m2", 7),
        norm_grow,
    );
    pair.reconnect(kafka, doze);
    pair.same::<_, CreatePartitionsResponse>(
        "mutations: permissive grow",
        ApiKey::CreatePartitions,
        2,
        &grow("oracle-m2", 7),
        norm_grow,
    );
    pair.reconnect(kafka, doze);
    pair.same::<_, CreateTopicsResponse>(
        "mutations: permissive create",
        ApiKey::CreateTopics,
        5,
        &create(&[("oracle-m5", 2)]),
        norm,
    );
    pair.reconnect(kafka, doze);
    let unset: &[Change<'_>] = &[(
        &[("client-id", Some("oracle-diff"))],
        &[("controller_mutation_rate", None)],
    )];
    let _: (AlterClientQuotasResponse, AlterClientQuotasResponse) =
        pair.both(ApiKey::AlterClientQuotas, 1, &alter(unset, false));
}
