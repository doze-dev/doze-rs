use super::*;
use crate::refusal::refusal;

fn text(lines: &Lines) -> String {
    lines
        .iter()
        .map(|l| l.iter().map(|(_, t)| t.as_str()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn the_create_call_is_the_boards_with_and_without_configs() {
    let bare = create_topics(7, "orders.retry", "3", &[], false);
    assert_eq!(
        text(&bare),
        "CreateTopics v7\n  topics[0]\n    name: orders.retry\n    num_partitions: 3\n    replication_factor: 1\n    configs: (none yet: all defaults)\n  validate_only: false"
    );
    let set = create_topics(
        7,
        "orders.retry",
        "3",
        &[("cleanup.policy", "delete"), ("retention.ms", "604800000")],
        true,
    );
    assert_eq!(
        text(&set),
        "CreateTopics v7\n  topics[0]\n    name: orders.retry\n    num_partitions: 3\n    replication_factor: 1\n    configs:\n      cleanup.policy = delete\n      retention.ms = 604800000\n  validate_only: true"
    );
    // Keywords and strings are coloured as the board colours them.
    assert_eq!(bare[0][0], (Tok::Keyword, "CreateTopics".to_owned()));
    assert_eq!(bare[2][1], (Tok::Str, "orders.retry".to_owned()));
    assert_eq!(set[6][1], (Tok::Str, "cleanup.policy".to_owned()));
}

#[test]
fn the_create_answer_carries_kafkas_code_and_sentence() {
    let r = refusal(36, Some("Topic 'orders.retry' already exists."));
    assert_eq!(
        text(&create_topics_answer(7, "orders.retry", &r)),
        "CreateTopics v7 response\n  topics[0]\n    name: orders.retry\n    error_code: 36 TOPIC_ALREADY_EXISTS\n    error_message: Topic 'orders.retry' already exists.\n    num_partitions: -1\n    replication_factor: -1"
    );
}

#[test]
fn the_produce_call_draws_the_record_and_the_answer_the_offset() {
    let headers = vec![("content-type".to_owned(), "application/json".to_owned())];
    let r = RecordText {
        key: Some("ord-88420"),
        value: Some("{\"order_id\":\"ord-88420\",\"customer\":\"c-3092\"}"),
        headers: &headers,
    };
    assert_eq!(
        text(&produce(9, "orders", 2, -1, &r)),
        "Produce v9\n  acks: -1\n  timeout_ms: 30000\n  topic_data[0]\n    name: orders\n    partition_data[0]\n      index: 2\n      records: 1 batch, 1 record\n        key: ord-88420\n        value: {\"order_id\":\"ord-88420\",\"custome ...\n        headers: content-type = application/json"
    );
    let keyless = RecordText {
        key: None,
        value: None,
        headers: &[],
    };
    let t = text(&produce(9, "t", 0, 1, &keyless));
    assert!(
        t.contains("key: (none)") && t.contains("value: (none: a deletion)"),
        "{t}"
    );
    assert!(!t.contains("headers"));
    assert_eq!(
        text(&produce_answer(
            9,
            "orders",
            2,
            Outcome {
                code: 0,
                name: "NONE",
                base_offset: 16_058
            },
            &[("log_append_time_ms", "-1".to_owned())]
        )),
        "Produce v9 response\n  topic: orders\n  partition: 2\n  error_code: 0\n  base_offset: 16058\n  log_append_time_ms: -1"
    );
    let refused = text(&produce_answer(
        9,
        "inventory.levels",
        0,
        Outcome {
            code: 87,
            name: "INVALID_RECORD",
            base_offset: -1,
        },
        &[],
    ));
    assert!(
        refused.contains("error_code: 87 INVALID_RECORD") && refused.contains("base_offset: -1"),
        "{refused}"
    );
}

#[test]
fn the_other_calls_are_the_boards() {
    assert_eq!(
        text(&create_partitions(3, "orders", "5", false)),
        "CreatePartitions v3\n  topics[0]\n    name: orders\n    count: 5\n    assignments: null\n  validate_only: false"
    );
    let r = refusal(
        37,
        Some("The topic orders currently has 3 partition(s); 2 would not be an increase."),
    );
    assert_eq!(
        text(&create_partitions_answer(3, "orders", &r)),
        "CreatePartitions v3 response\n  name: orders\n  error_code: 37 INVALID_PARTITIONS\n  error_message: The topic orders currently has 3 partition(s); 2 would not be an increase."
    );
    assert_eq!(
        text(&delete_records(2, "orders", 0, "16000")),
        "DeleteRecords v2\n  topics[0]\n    name: orders\n    partitions[0]\n      partition_index: 0\n      offset: 16000\n  timeout_ms: 30000"
    );
    let p = refusal(44, None);
    assert_eq!(
        text(&delete_records_answer(2, "inventory.levels", 0, -1, &p)),
        "DeleteRecords v2 response\n  topic: inventory.levels\n  partition_index: 0\n  low_watermark: -1\n  error_code: 44 POLICY_VIOLATION"
    );
    assert_eq!(
        text(&delete_topics(6, "orders")),
        "DeleteTopics v6\n  topics[0]\n    name: orders"
    );
}

#[test]
fn what_was_typed_cannot_make_a_second_line_or_run_on() {
    assert_eq!(visible("a\nb\r\0c", 40), "a\\nb\\r\\u{0}c");
    assert_eq!(visible("abcdef", 3), "abc ...");
    assert_eq!(visible("abc", 3), "abc");
    assert_eq!(visible("\u{e9}\u{e9}\u{e9}", 2), "\u{e9}\u{e9} ...");
    let t = text(&create_topics(
        7,
        "x\ny: z",
        "1\n2",
        &[("k\nj", "v\nw")],
        false,
    ));
    assert_eq!(t.lines().count(), 8, "{t}");
    let long = "a".repeat(10_000);
    assert!(text(&create_topics(7, &long, "1", &[], false)).len() < 400);
}

#[test]
fn a_shell_word_is_bare_when_safe_and_quoted_when_not() {
    for (raw, want) in [
        ("orders.retry", "orders.retry"),
        ("127.0.0.1:9092", "127.0.0.1:9092"),
        ("cleanup.policy=delete", "cleanup.policy=delete"),
        ("", "''"),
        ("a b", "'a b'"),
        ("it's", "'it'\\''s'"),
        ("$(rm -rf /)", "'$(rm -rf /)'"),
        ("a\nb", "'a b'"),
        ("`x`", "'`x`'"),
        ("a;b", "'a;b'"),
    ] {
        assert_eq!(shell_word(raw), want, "{raw:?}");
    }
}

#[test]
fn the_terminal_commands_are_the_boards() {
    assert_eq!(
        create_command(
            "127.0.0.1:9092",
            "orders.retry",
            "3",
            &[("cleanup.policy", "delete"), ("retention.ms", "604800000")]
        )
        .join("\n"),
        "kafka-topics.sh --bootstrap-server 127.0.0.1:9092 \\\n  --create --topic orders.retry --partitions 3 \\\n  --config cleanup.policy=delete \\\n  --config retention.ms=604800000"
    );
    assert_eq!(
        create_command("127.0.0.1:9092", "t", "1", &[]).join("\n"),
        "kafka-topics.sh --bootstrap-server 127.0.0.1:9092 \\\n  --create --topic t --partitions 1"
    );
    assert_eq!(
        kcat_produce("127.0.0.1:9092", "orders.retry").join("\n"),
        "echo '{\"order_id\":\"ord-88420\"}' \\\n  | kcat -P -b 127.0.0.1:9092 \\\n    -t orders.retry -k ord-88420"
    );
    assert_eq!(
        kcat_consume("127.0.0.1:9092", "orders.retry").join("\n"),
        "kcat -C -b 127.0.0.1:9092 \\\n  -G orders-retry-workers orders.retry"
    );
    // A hostile name is one quoted word.
    assert!(
        create_command("a", "x;rm -rf /", "1", &[])
            .join(" ")
            .contains("'x;rm -rf /'")
    );
}
