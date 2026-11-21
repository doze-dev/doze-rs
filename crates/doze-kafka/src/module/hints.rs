//! What protocol 1's core hard-coded for kafka and the module declares instead: the shell client, and how `doze init`
//! recognises a project that uses Kafka.

use std::collections::BTreeMap;

use doze_module::proto::msg::hello::{ClientCommand, Detect};

/// The shell client: kcat at the broker's address, listing what it holds (brokers, topics, partitions). Its other
/// modes take the same `-b` (`kcat -b … -t orders -C`), so the line doze prints is the one to start from.
pub(super) fn client() -> ClientCommand {
    ClientCommand {
        program: "kcat".into(),
        args: ["-b", "{host}:{port}", "-L"].map(String::from).to_vec(),
        env: BTreeMap::new(),
        fallbacks: vec!["kafkacat".into()],
    }
}

/// How `doze init` recognises a project that uses Kafka (protocol 1's core, `internal/detect/detect.go`).
pub(super) fn detect() -> Detect {
    let list = |v: &[&str]| v.iter().map(ToString::to_string).collect::<Vec<_>>();
    Detect {
        packages: BTreeMap::from([
            (
                "go".into(),
                list(&[
                    "github.com/twmb/franz-go",
                    "github.com/segmentio/kafka-go",
                    "github.com/IBM/sarama",
                    "github.com/Shopify/sarama",
                    "github.com/confluentinc/confluent-kafka-go",
                ]),
            ),
            ("npm".into(), list(&["kafkajs", "node-rdkafka"])),
        ]),
        images: list(&[
            "confluentinc/cp-kafka",
            "bitnami/kafka",
            "apache/kafka",
            "redpandadata/redpanda",
        ]),
        env: BTreeMap::from([
            ("KAFKA_BROKERS".into(), "address".into()),
            ("KAFKA_BOOTSTRAP_SERVERS".into(), "address".into()),
            ("KAFKA_URL".into(), "address".into()),
        ]),
    }
}
