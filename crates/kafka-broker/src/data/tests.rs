use std::collections::BTreeMap;

use super::*;
use crate::Defaults;

fn keeps(own: &[(&str, &str)], defaults: Defaults) -> Keeps {
    let own: BTreeMap<String, String> = own
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    Keeps::of(&defaults, &own)
}

fn open() -> Defaults {
    Defaults {
        retention_bytes: -1,
        retention_ms: -1,
        ..Defaults::default()
    }
}

#[test]
fn a_time_is_said_in_the_largest_unit_it_is_a_whole_number_of() {
    for (ms, said) in [
        (604_800_000, "7 days"),
        (86_400_000, "1 day"),
        (3_600_000, "1 hour"),
        (5_400_000, "90 minutes"),
        (45_000, "45 seconds"),
        (250, "250 ms"),
    ] {
        assert_eq!(duration_words(ms), said);
    }
}

#[test]
fn bytes_are_said_in_powers_of_1024_with_a_tenth_below_ten() {
    for (n, said) in [
        (0, "0 B"),
        (1023, "1023 B"),
        (1536, "1.5 KiB"),
        (128 << 20, "128 MiB"),
        (1 << 30, "1.0 GiB"),
        (5 << 40, "5.0 TiB"),
    ] {
        assert_eq!(bytes_words(n), said);
    }
}

#[test]
fn what_a_topic_keeps_comes_from_its_own_configs_then_the_brokers() {
    let k = keeps(&[], open());
    assert!(k.unbounded() && !k.own);
    let k = keeps(&[("retention.ms", "3600000")], open());
    assert_eq!((k.max_age_ms, k.own), (Some(3_600_000), true));
    let by_default = Defaults {
        retention_bytes: 1 << 30,
        ..open()
    };
    let k = keeps(&[], by_default);
    assert_eq!((k.max_bytes, k.own), (Some(1 << 30), false));
    // A topic that only compacts is not touched by retention, whatever it says.
    let k = keeps(
        &[("cleanup.policy", "compact"), ("retention.ms", "1")],
        open(),
    );
    assert_eq!((k.compact, k.delete, k.max_age_ms), (true, false, None));
    assert!(!k.unbounded());
    // -1 is no bound.
    assert!(keeps(&[("retention.ms", "-1")], open()).unbounded());
}

#[test]
fn why_says_what_is_kept_and_when_the_oldest_goes() {
    /// A topic's own configs, the wait until the oldest goes, and the sentence.
    type Case<'a> = (&'a [(&'a str, &'a str)], Option<i64>, &'a str);
    let cases: [Case<'_>; 7] = [
        (
            &[],
            None,
            "No retention limit: it keeps everything written to it, for ever.",
        ),
        (
            &[("cleanup.policy", "compact")],
            None,
            "Compacted: it keeps the latest value of every key, so it grows with the number of keys, not with time.",
        ),
        (
            &[("cleanup.policy", "compact,delete")],
            None,
            "Compacted with delete, but no retention limit: it keeps the latest value of every key, for ever.",
        ),
        (
            &[("retention.ms", "604800000")],
            Some(3 * 86_400_000 + 1_000),
            "Retention 7 days: whole segments past it are deleted. The oldest goes in about 3 days.",
        ),
        (
            &[("retention.ms", "604800000")],
            Some(-5),
            "Retention 7 days: whole segments past it are deleted. The oldest is due at the next sweep.",
        ),
        (
            &[("retention.bytes", "1073741824")],
            Some(1_000),
            "Retention 1.0 GiB per partition: whole segments past it are deleted.",
        ),
        (
            &[
                ("retention.ms", "3600000"),
                ("retention.bytes", "1048576"),
                ("cleanup.policy", "compact,delete"),
            ],
            Some(90 * 60_000),
            "Retention 1 hour or 1.0 MiB per partition, whichever comes first: compacted, and whole segments past it are deleted. The oldest goes in about 90 minutes.",
        ),
    ];
    for (own, due, said) in cases {
        assert_eq!(keeps(own, open()).why(due), said, "{own:?}");
    }
}

#[test]
fn a_refusal_names_its_kafka_error_and_says_what_to_do() {
    let denied = DataRefusal::Denied {
        right: "ALTER on the cluster".into(),
        cluster: true,
    };
    assert_eq!(denied.error(), ResponseError::ClusterAuthorizationFailed);
    assert!(denied.sentence().contains("ALTER on the cluster"));
    let busy = DataRefusal::TxnOpen {
        topic: None,
        id: Some("payments".into()),
    };
    assert_eq!(busy.error(), ResponseError::ConcurrentTransactions);
    assert_eq!(
        busy.sentence(),
        "A transaction is under way on this broker (transactional id payments). Emptying now would cut it in half, so nothing was changed: commit or abort it (or wait for its timeout), then try again."
    );
}
