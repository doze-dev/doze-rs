use super::*;
use crate::model::ConfigSynonym;

fn row(name: &str, value: &str, source: ConfigSource) -> ConfigRow {
    ConfigRow {
        name: name.to_owned(),
        value: value.to_owned(),
        source,
        kind: 5,
        doc: String::new(),
        allowed: String::new(),
        synonyms: Vec::new(),
    }
}

#[test]
fn a_value_is_said_in_words_where_it_helps_and_left_alone_where_it_does_not() {
    for (key, value, want) in [
        ("retention.ms", "-1", Some("no time limit")),
        ("retention.ms", "604800000", Some("7 d")),
        ("retention.bytes", "-1", Some("no size limit")),
        ("flush.ms", "9223372036854775807", Some("no limit")),
        (
            "max.compaction.lag.ms",
            "9223372036854775807",
            Some("no limit"),
        ),
        ("min.compaction.lag.ms", "0", Some("no waiting")),
        ("min.compaction.lag.ms", "7200000", Some("2 h")),
        ("segment.bytes", "8388608", Some("8 MiB")),
        ("segment.bytes", "134217728", Some("128 MiB")),
        ("max.message.bytes", "1048588", Some("about 1 MiB")),
        ("segment.bytes", "1000", None),
        ("segment.jitter.ms", "0", None),
        ("min.cleanable.dirty.ratio", "0.5", None),
        ("compression.type", "producer", None),
        ("segment.bytes", "abc", None),
        ("segment.bytes", "", None),
        ("delete.retention.ms", "86400000", Some("1 d")),
    ] {
        assert_eq!(gloss(key, value).as_deref(), want, "{key} {value}");
    }
}

#[test]
fn a_policy_says_what_it_does() {
    assert_eq!(
        policy_gloss("compact"),
        Some("keep the latest record of each key")
    );
    assert_eq!(policy_gloss("delete"), Some("discard old segments"));
    assert!(policy_gloss("delete, compact").unwrap().contains("trim"));
    assert_eq!(policy_gloss("compacted"), None);
    assert_eq!(policy_gloss(""), None);
}

#[test]
fn a_value_set_on_the_topic_names_the_flag_it_hides() {
    let mut r = row("segment.bytes", "8388608", ConfigSource::DynamicTopic);
    r.synonyms = vec![
        ConfigSynonym {
            name: "segment.bytes".into(),
            value: "8388608".into(),
            source: ConfigSource::DynamicTopic,
        },
        ConfigSynonym {
            name: "log.segment.bytes".into(),
            value: "134217728".into(),
            source: ConfigSource::StaticFlag,
        },
    ];
    assert_eq!(row_gloss(&r).as_deref(), Some("8 MiB; the flag is 128 MiB"));
    r.source = ConfigSource::Default;
    assert_eq!(row_gloss(&r).as_deref(), Some("8 MiB"));
}

#[test]
fn the_thirty_three_keys_fall_in_the_groups_the_board_counts() {
    let keys = include_str!("../config_effects/tests.rs");
    let listed: Vec<&str> = keys
        .lines()
        .skip_while(|l| !l.starts_with("const LISTED"))
        .skip(1)
        .take_while(|l| !l.starts_with("];"))
        .filter_map(|l| l.trim().strip_prefix('"')?.strip_suffix("\","))
        .collect();
    assert_eq!(listed.len(), 33);
    let count = |g: Group| listed.iter().filter(|k| group_of(k) == g).count();
    // The board's counts, with segment.bytes, cleanup.policy, min.compaction.lag.ms and retention.ms (4) in "Changed".
    assert_eq!(count(Group::Cleanup), 5);
    assert_eq!(count(Group::Retention), 2);
    assert_eq!(count(Group::Segments), 7);
    assert_eq!(count(Group::Records), 8);
    assert_eq!(count(Group::Durability), 2);
    assert_eq!(count(Group::Replication), 9);
    assert_eq!(group_of("a.key.nobody.knows"), Group::Records);
}

#[test]
fn the_chips_name_their_groups() {
    assert!(is_compaction("min.cleanable.dirty.ratio") && !is_compaction("segment.bytes"));
    assert!(is_segments("segment.bytes") && !is_segments("retention.ms"));
}

#[test]
fn kafkas_text_is_plain_text_and_its_first_sentence_is_the_row() {
    let doc = "Determines whether <code>x</code> is on.  This is <b>more</b>.\n Even more.";
    assert_eq!(
        plain_text(doc),
        "Determines whether x is on. This is more. Even more."
    );
    assert_eq!(first_sentence(doc), "Determines whether x is on.");
    assert!(has_more(doc));
    assert_eq!(first_sentence("One sentence"), "One sentence");
    assert!(!has_more("One sentence."));
    assert_eq!(plain_text("<script>alert(1)</script>"), "alert(1)");
    assert_eq!(first_sentence(""), "");
}

#[test]
fn the_sentence_under_the_name_counts_the_sources() {
    let rows = vec![
        row("a", "1", ConfigSource::DynamicTopic),
        row("b", "1", ConfigSource::StaticFlag),
        row("c", "1", ConfigSource::Default),
        row("d", "1", ConfigSource::Default),
    ];
    assert_eq!(tally(&rows), (1, 1, 2));
    let text: String = page_sentence(&rows, "Tail.")
        .iter()
        .map(|s| match s {
            Span::Text(t) | Span::Mono(t) | Span::Strong(t) => t.as_str(),
        })
        .collect();
    assert_eq!(
        text,
        "4 configs. 1 are set on this topic, 1 comes from a broker flag, 2 are Kafka's defaults. Tail."
    );
    assert_eq!(type_word(7), "list");
    assert_eq!(type_word(99), "other");
}
