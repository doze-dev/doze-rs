//! The fourteen boards of the topic writes, rendered from the "larder" fixtures with no broker: written to
//! `target/board-diff/<id>.html` for `tools/board-diff.ts`, and checked here against what the boards say (their headings,
//! their labels, their buttons, their numbers) so that a page that drifts from its board fails before the tool runs.
#![allow(
    clippy::too_many_lines,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::path::PathBuf;

use kafka_console::base::Base;
use kafka_console::boards_write::{IDS, render};

fn out_dir() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest.parent().unwrap().parent().unwrap();
    let target =
        std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| root.join("target"), PathBuf::from);
    target.join("board-diff")
}

/// Every text of `want` appears in `doc`, in this order.
fn in_order(id: &str, doc: &str, want: &[&str]) {
    let mut from = 0;
    for text in want {
        let at = doc[from..]
            .find(text)
            .unwrap_or_else(|| panic!("{id}: {text:?} is not on the page after byte {from}"));
        from += at + text.len();
    }
}

fn page(id: &str) -> String {
    render(id, &Base::root()).unwrap_or_else(|| panic!("no render for {id}"))
}

#[test]
fn every_board_of_the_slice_renders_and_is_written_for_the_tool() {
    let dir = out_dir();
    std::fs::create_dir_all(&dir).unwrap();
    for id in IDS {
        let doc = page(id);
        assert!(doc.starts_with("<!DOCTYPE html>"), "{id}");
        std::fs::write(dir.join(format!("{id}.html")), &doc).unwrap();
    }
    assert_eq!(IDS.len(), 14);
    assert!(
        render("topics-list", &Base::root()).is_none(),
        "a board of the read builder is not drawn here"
    );
}

#[test]
fn the_three_steps_of_create_have_the_boards_parts_in_their_order() {
    in_order(
        "topic-create",
        &page("topic-create"),
        &[
            "<h1>Create topic</h1>",
            "Three steps. The call on the right is the one that will be sent.",
            "Name and partitions",
            "Configuration",
            "Review and create",
            "<label for=\"n\">Name</label>",
            "orders.retry",
            "<b>Kafka says</b> legal characters are ASCII letters and digits",
            "<label for=\"p\">Partitions</label>",
            "of 1 to 1,000",
            "Start from",
            "Event log",
            "Compacted state",
            "Short lived",
            "Cancel",
            "Next: configuration",
            "<div class=\"hd\">",
            "The call",
            "CreateTopics",
            " v7",
            "configs: ",
            "(none yet: all defaults)",
            "validate_only: ",
            "false",
            "Or from a terminal",
            "kafka-topics.sh --bootstrap-server 127.0.0.1:9092",
            "Copy",
        ],
    );
    in_order(
        "topic-create-config",
        &page("topic-create-config"),
        &[
            "cleanup.policy",
            "delete",
            "compact",
            "delete,compact",
            "<b>Kafka says</b> The &quot;delete&quot; policy",
            "<label for=\"r\">retention.ms</label>",
            "604800000",
            "= 7 d",
            "Back",
            "Next: review",
            "--config cleanup.policy=delete",
        ],
    );
    in_order(
        "topic-create-review",
        &page("topic-create-review"),
        &[
            "What you will get",
            "<dt>Name</dt>",
            "orders.retry",
            "<dt>Partitions</dt>",
            "directories made and flushed before the topic is announced",
            "<dt>Replication factor</dt>",
            "one broker, said once",
            "<dt>cleanup.policy</dt>",
            "<dt>retention.ms</dt>",
            "(7 d)",
            "<dt>Everything else</dt>",
            "Back",
            "Check only",
            "Create topic",
            "What can refuse this call",
        ],
    );
    let review = page("topic-create-review");
    assert_eq!(
        review.matches("data-op=\"CreateTopics\"").count(),
        2,
        "Check only and Create topic"
    );
}

#[test]
fn the_refused_create_names_kafkas_code_and_what_would_pass() {
    in_order(
        "topic-create-refused",
        &page("topic-create-refused"),
        &[
            "The topic was not created.",
            "Refused · TOPIC_ALREADY_EXISTS (36)",
            "Topic 'orders.retry' already exists.",
            "What would pass",
            "another name",
            "Open the existing topic",
            "Choose another name",
            "Back to topics",
            "The other refusals of this call",
            "INVALID_TOPIC_EXCEPTION (17)",
            "INVALID_PARTITIONS (37)",
            "INVALID_CONFIG (40)",
            "The answer",
            "error_code: ",
            "36 TOPIC_ALREADY_EXISTS",
            "num_partitions: ",
            "-1",
        ],
    );
}

#[test]
fn the_created_topic_is_empty_and_says_what_next() {
    in_order(
        "topic-created",
        &page("topic-created"),
        &[
            "Created orders.retry.",
            "3 partitions, cleanup.policy delete, retention 7 d",
            "It is on disk now",
            "<h1 class=\"mono\">orders.retry</h1>",
            "Empty: offsets 0 to 0 on all 3 partitions.",
            "Produce a record",
            "Open topic",
            "Next steps",
            "1 · Produce",
            "kcat -P -b 127.0.0.1:9092",
            "2 · Add a consumer",
            "-G orders-retry-workers orders.retry",
            "Open Groups",
        ],
    );
}

#[test]
fn the_produce_dialog_and_its_two_results() {
    in_order(
        "topic-produce",
        &page("topic-produce"),
        &[
            "role=\"dialog\"",
            "Produce to <span class=\"mono\">orders</span>",
            "Partition",
            "Key",
            "optional on this topic",
            "ord-88420",
            "Value",
            "Header",
            "content-type",
            "acks",
            "all (-1)",
            "Kafka says",
            "plain write, not part of transaction",
            "orders-eos",
            "keeps its records by delete, so a record with no key is accepted",
            "The call",
            "Produce",
            " v9",
            "index: ",
            "2",
            "Cancel",
            "Send",
        ],
    );
    in_order(
        "topic-produce-keyless",
        &page("topic-produce-keyless"),
        &[
            "<h1 class=\"mono\">inventory.levels</h1>",
            "compact",
            "A compacted topic keeps the latest record for every key.",
            "Produce to inventory.levels",
            "required on this topic",
            "A compacted topic needs a key on every record.",
            "Sent once: the broker answered below.",
            "Refused · INVALID_RECORD (87)",
            "Compacted topic cannot accept message without key in topic partition inventory.levels-0",
            "What would pass",
            "give the record a key",
            "tombstone",
            "The answer",
            "87 INVALID_RECORD",
            "Back to Produce",
        ],
    );
    in_order(
        "topic-produced",
        &page("topic-produced"),
        &[
            "Sent to orders-2 at offset 16,058.",
            "22:14:41.208",
            "acknowledged after the flush (acks -1)",
            "<h1 class=\"mono\">orders</h1>",
            "Transaction open",
            "orders</b> has 3 partitions, keeps records for 7 d, holds <b>48,214</b> records",
            "A transaction is open on <b>orders-0 and orders-1</b>",
            "held 6 records behind on orders-0",
            "Partitions",
            "Produce",
            "Delete topic",
            "Messages",
            "Log",
            "Config",
            "Consumers",
            "Producers",
            "For read_committed",
            "Committed",
            "Above LSO",
            "Newest first · 5 of 48,214",
            "The answer",
            "base_offset: ",
            "16058",
            "Is it visible?",
        ],
    );
}

#[test]
fn partitions_records_and_topics_are_dialogs_with_the_boards_words() {
    in_order(
        "topic-add-partitions",
        &page("topic-add-partitions"),
        &[
            "Add partitions to <span class=\"mono\">orders</span>",
            "Partitions after",
            "now 3",
            "adds 2: orders-3 and orders-4",
            "<b>Kafka says</b> the new total, not the number to add.",
            "it must be above 3",
            "4 to 1,000",
            "Replica assignment",
            "ignored: this broker is the only replica",
            "Never fewer.",
            "CreatePartitions",
            " v3",
            "Cancel",
            "Add 2 partitions",
        ],
    );
    in_order(
        "topic-add-partitions-refused",
        &page("topic-add-partitions-refused"),
        &[
            "2 is below the 3 partitions there are.",
            "Refused · INVALID_PARTITIONS (37)",
            "The topic orders currently has 3 partition(s); 2 would not be an increase.",
            "What would pass",
            "any total from 4 to 1,000.",
            "The answer",
            "37 INVALID_PARTITIONS",
            "Cancel",
            "Change the count",
        ],
    );
    in_order(
        "topic-delete-records",
        &page("topic-delete-records"),
        &[
            "Delete records from <span class=\"mono\">orders-0</span>",
            "Delete everything before offset",
            "16000",
            "log start now 0 · end 16,104",
            "This removes 16,000 records",
            "offsets 0 to 15,999 of orders-0",
            "Offset 16,000 stays",
            "payments-workers",
            "is at 16,092: it is not affected.",
            "DeleteRecords",
            " v2",
            "Cancel",
            "Delete 16,000 records",
        ],
    );
    in_order(
        "topic-delete-records-refused",
        &page("topic-delete-records-refused"),
        &[
            "<h1 class=\"mono\">inventory.levels</h1>",
            "Delete records on a topic that only compacts.",
            "log start is still 0.",
            "Refused · POLICY_VIOLATION (44)",
            "Request parameters do not satisfy the configured policy.",
            "What would pass",
            "delete,compact",
            "What removes records here instead",
            "The cleaner",
            "A tombstone",
            "Retention",
            "See what compaction did to this log",
            "The answer",
            "44 POLICY_VIOLATION",
        ],
    );
    in_order(
        "topic-delete",
        &page("topic-delete"),
        &[
            "Delete <span class=\"mono\">orders</span>?",
            "This removes <b>48,213 records</b> in 3 partitions",
            "It cannot be undone",
            "It is in use.",
            "payments-workers",
            "transaction <span class=\"mono\">orders-eos</span>",
            "Type the name to confirm",
            "DeleteTopics",
            " v6",
            "Cancel",
            "Delete topic",
        ],
    );
    in_order(
        "topic-deleted",
        &page("topic-deleted"),
        &[
            "Deleted orders.",
            "48,213 records and 3 partitions are gone from disk",
            "<h1>Topics</h1>",
            "8 topics, 18 partitions.",
            "Create topic",
            "What happens next.",
            "A topic named <span class=\"mono\">orders</span> made later is a new topic",
            "Create it again",
            "Name",
            "Partitions",
            "Cleanup policy",
            "Size on disk",
            "Last write",
            "payments.settled",
            "orders.dlq",
            "empty: no records yet",
        ],
    );
}

#[test]
fn the_dialogs_are_labelled_and_every_label_is_bound() {
    for id in [
        "topic-produce",
        "topic-add-partitions",
        "topic-add-partitions-refused",
        "topic-delete-records",
        "topic-delete",
    ] {
        let doc = page(id);
        assert!(
            doc.contains("role=\"dialog\" aria-modal=\"true\" aria-labelledby=\""),
            "{id}"
        );
        let title = doc
            .split("aria-labelledby=\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap();
        assert!(
            doc.contains(&format!("<h2 id=\"{title}\">")),
            "{id}: the dialog is labelled by its title"
        );
    }
}
