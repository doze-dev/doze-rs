use super::*;
use crate::model::{ConfigOp, ConfigSynonym};

fn row(name: &str, value: &str, source: ConfigSource, kind: i8, allowed: &str) -> ConfigRow {
    ConfigRow {
        name: name.to_owned(),
        value: value.to_owned(),
        source,
        kind,
        doc: "Kafka says this. And more.".to_owned(),
        allowed: allowed.to_owned(),
        synonyms: vec![ConfigSynonym {
            name: name.to_owned(),
            value: value.to_owned(),
            source,
        }],
    }
}

fn input(key: &str, value: &str, other: &str) -> ConfigInput {
    ConfigInput {
        key: key.to_owned(),
        op: ConfigOp::Set,
        value: value.to_owned(),
        other: other.to_owned(),
    }
}

fn text(parts: &[Span]) -> String {
    parts
        .iter()
        .map(|s| match s {
            Span::Text(t) | Span::Mono(t) | Span::Strong(t) => t.as_str(),
        })
        .collect()
}

const PACE: ConfigPace = ConfigPace {
    cleaner_secs: 15,
    cleaner_on: true,
    retention_secs: 300,
};

#[test]
fn a_single_value_is_taught_from_the_row_and_the_effects_table() {
    let r = row(
        "min.cleanable.dirty.ratio",
        "0.5",
        ConfigSource::Default,
        6,
        "a number from 0 to 1",
    );
    let t = teach(
        &r,
        None,
        ConfigForm::Single,
        &input("min.cleanable.dirty.ratio", "0.7", ""),
        Some(PACE),
        None,
    );
    assert_eq!(
        text(&t.what),
        "how much of the log may be dirty before the cleaner takes it"
    );
    assert_eq!(
        text(&t.now),
        "0.5, Kafka's default; nothing is set on this topic"
    );
    assert_eq!(text(&t.allowed), "a number from 0 to 1");
    assert_eq!(
        text(&t.takes_effect),
        "at the cleaner's next look, within 15 s"
    );
    assert!(
        t.here.is_empty(),
        "nothing is said of the log that the broker did not say"
    );
}

#[test]
fn a_key_the_boards_do_not_teach_is_taught_in_kafkas_words() {
    let r = row(
        "preallocate",
        "false",
        ConfigSource::Default,
        1,
        "true or false",
    );
    let t = teach(
        &r,
        None,
        ConfigForm::Single,
        &input("preallocate", "true", ""),
        None,
        None,
    );
    assert_eq!(text(&t.what), "Kafka says this. And more.");
    assert!(text(&t.takes_effect).starts_with("never"));
}

#[test]
fn the_lags_are_taught_together_and_here_says_what_the_typed_pair_means() {
    let min = row(
        MIN_LAG,
        "0",
        ConfigSource::DynamicTopic,
        5,
        "a whole number, at least 0",
    );
    let max = row(
        MAX_LAG,
        "9223372036854775807",
        ConfigSource::Default,
        5,
        "a whole number, at least 1",
    );
    let good = teach(
        &min,
        Some(&max),
        ConfigForm::Lags,
        &input(MIN_LAG, "3600000", "7200000"),
        Some(PACE),
        None,
    );
    assert_eq!(
        text(&good.now),
        "min 0 set on this topic; max 9223372036854775807 not set (no limit)"
    );
    assert!(text(&good.allowed).contains("min not above max"));
    assert_eq!(
        text(&good.here),
        "with min 1 h a record is left alone for 1 h; with max 2 h it would be forced after 2 h"
    );
    // A crossed pair, or text, is not explained: the broker will say what is wrong with it.
    for (a, b) in [
        ("7200000", "3600000"),
        ("x", "1"),
        ("1", ""),
        ("-1", "5"),
        ("1", "0"),
    ] {
        let t = teach(
            &min,
            Some(&max),
            ConfigForm::Lags,
            &input(MIN_LAG, a, b),
            Some(PACE),
            None,
        );
        assert!(t.here.is_empty(), "{a} {b}");
    }
}

#[test]
fn a_cleanup_policy_says_what_adding_delete_does_with_the_retention_it_has() {
    let r = row(
        "cleanup.policy",
        "compact",
        ConfigSource::DynamicTopic,
        7,
        "compact or delete, each element checked",
    );
    let t = teach(
        &r,
        None,
        ConfigForm::List,
        &input("cleanup.policy", "delete", ""),
        Some(PACE),
        Some(86_400_000),
    );
    let here = text(&t.here);
    assert!(
        here.contains("keep the latest record of each key"),
        "{here}"
    );
    assert!(
        here.contains("retention (1 d) trims this topic too"),
        "{here}"
    );
    let forever = teach(
        &r,
        None,
        ConfigForm::List,
        &input("cleanup.policy", "delete", ""),
        None,
        Some(-1),
    );
    assert!(text(&forever.here).contains("keeps records for ever"));
}
