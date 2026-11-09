use super::*;

fn one(name: &str, value: &str) -> BTreeMap<String, String> {
    BTreeMap::from([(name.to_owned(), value.to_owned())])
}

#[test]
fn a_bad_config_is_refused_in_the_words_apache_kafka_uses() {
    // Recorded from Apache Kafka 4.3.1 (`createtopics-validation`), but for segment.bytes, whose floor here is
    // 1024 so that retention can be tried on a handful of records.
    let cases = [
        (
            "no.such.config",
            "1",
            "Unknown topic config name: no.such.config",
        ),
        (
            "retention.ms",
            "abc",
            "Invalid value abc for configuration retention.ms: Not a number of type LONG",
        ),
        (
            "segment.bytes",
            "1",
            "Invalid value 1 for configuration segment.bytes: Value must be at least 1024",
        ),
        (
            "segment.bytes",
            "99999999999",
            "Invalid value 99999999999 for configuration segment.bytes: Not a number of type INT",
        ),
        (
            "cleanup.policy",
            "bogus",
            "Invalid value bogus for configuration cleanup.policy: String must be one of: compact, delete",
        ),
        (
            "compression.type",
            "zip",
            "Invalid value zip for configuration compression.type: String must be one of: uncompressed, zstd, lz4, snappy, gzip, producer",
        ),
        (
            "min.insync.replicas",
            "0",
            "Invalid value 0 for configuration min.insync.replicas: Value must be at least 1",
        ),
        (
            "max.message.bytes",
            "-1",
            "Invalid value -1 for configuration max.message.bytes: Value must be at least 0",
        ),
        (
            "message.timestamp.type",
            "WallClock",
            "Invalid value WallClock for configuration message.timestamp.type: String must be one of: CreateTime, LogAppendTime",
        ),
        (
            "preallocate",
            "yes",
            "Invalid value yes for configuration preallocate: String must be one of: true, false",
        ),
        (
            "min.cleanable.dirty.ratio",
            "1.5",
            "Invalid value 1.5 for configuration min.cleanable.dirty.ratio: Value must be no more than 1",
        ),
        (
            "min.cleanable.dirty.ratio",
            "x",
            "Invalid value x for configuration min.cleanable.dirty.ratio: Not a number of type DOUBLE",
        ),
    ];
    for (name, value, message) in cases {
        assert_eq!(
            validate(&one(name, value)),
            Err(message.to_owned()),
            "{name}={value}"
        );
    }
}

#[test]
fn good_values_pass_and_the_first_bad_key_in_name_order_is_the_one_reported() {
    for (name, value) in [
        ("retention.ms", "3600000"),
        ("retention.ms", "-1"),
        ("cleanup.policy", "compact,delete"),
        ("cleanup.policy", "delete,delete"),
        ("segment.bytes", "1024"),
        ("min.cleanable.dirty.ratio", "0.25"),
        ("message.format.version", "anything"),
    ] {
        assert_eq!(validate(&one(name, value)), Ok(()), "{name}={value}");
    }
    let two = BTreeMap::from([
        ("segment.bytes".to_owned(), "1".to_owned()),
        ("retention.ms".to_owned(), "x".to_owned()),
    ]);
    assert!(validate(&two).unwrap_err().contains("retention.ms"));
}

#[test]
fn a_topics_configuration_is_the_broker_defaults_with_its_own_laid_over() {
    let defaults = Defaults {
        retention_ms: 5000,
        segment_bytes: 4096,
        ..Defaults::default()
    };
    let all = effective(&defaults, &one("retention.bytes", "100"));
    let get = |name: &str| {
        all.iter()
            .find(|e| e.name == name)
            .map(|e| (e.value.as_str(), e.source))
    };
    assert_eq!(get("retention.bytes"), Some(("100", Source::DynamicTopic)));
    // A value the broker was started with that is not Kafka's default is a static broker config.
    assert_eq!(get("retention.ms"), Some(("5000", Source::StaticBroker)));
    assert_eq!(get("segment.bytes"), Some(("4096", Source::StaticBroker)));
    assert_eq!(get("cleanup.policy"), Some(("delete", Source::Default)));
    // Reported in the order Kafka lists them, and a config with no default is reported only when set.
    assert_eq!(all[0].name, "compression.type");
    assert_eq!(get("preallocate"), Some(("false", Source::Default)));
    assert_eq!(get("message.downconversion.enable"), None);
    let set = effective(&defaults, &one("message.downconversion.enable", "true"));
    assert!(
        set.iter()
            .any(|e| e.name == "message.downconversion.enable")
    );
    // "No limit" is Kafka's -1, not this broker's zero.
    assert_eq!(
        effective(&Defaults::default(), &BTreeMap::new())
            .iter()
            .find(|e| e.name == "retention.bytes")
            .map(|e| e.value.as_str()),
        Some("-1")
    );
}
