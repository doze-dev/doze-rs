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

mod pair;
mod quotas;
mod requests;

use kafka_protocol::messages::delete_records_request::{
    DeleteRecordsPartition, DeleteRecordsTopic,
};
use kafka_protocol::messages::{
    ApiKey, CreateTopicsResponse, DeleteRecordsRequest, DeleteRecordsResponse,
    DescribeLogDirsResponse, OffsetForLeaderEpochResponse,
};

use pair::{Conn, Pair};
use quotas::{client_quotas, mutation_quota};
use requests::{Ask, create, epochs, log_dirs, norm_log_dirs, topic};

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
