//! The eleven boards of slice 9b that the reading half builds, rendered from the "larder" fixtures with no broker: written to
//! `target/board-diff/<id>.html` for `tools/board-diff.ts`, and checked here against what the boards say (headings, labels,
//! numbers) so that a page that drifts from its board fails before the tool runs.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::path::PathBuf;

use kafka_console::base::Base;
use kafka_console::boards::{TOPICS_READ_IDS, render_topics_read};

fn out_dir() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest.parent().unwrap().parent().unwrap();
    let target =
        std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| root.join("target"), PathBuf::from);
    target.join("board-diff")
}

fn doc(id: &str) -> String {
    render_topics_read(id, &Base::root()).unwrap_or_else(|| panic!("no render for {id}"))
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

#[test]
fn every_board_of_the_reading_half_renders_and_is_written_for_the_tool() {
    let dir = out_dir();
    std::fs::create_dir_all(&dir).unwrap();
    for id in TOPICS_READ_IDS {
        let d = doc(id);
        assert!(d.starts_with("<!DOCTYPE html>"), "{id}");
        std::fs::write(dir.join(format!("{id}.html")), &d).unwrap();
    }
    assert!(render_topics_read("home", &Base::root()).is_none());
}

#[test]
fn the_list_has_the_boards_nine_topics_and_the_hidden_internal_one() {
    let d = doc("topics-list");
    in_order(
        "topics-list",
        &d,
        &[
            "<h1>Topics</h1>",
            "9 topics hold",
            "<b>21</b>",
            "partitions.",
            "The newest write was <b>0.2 s</b> ago, to clickstream.",
            "One internal topic is hidden.",
            "Filter topics by name",
            "Show internal",
            "Refresh",
            "Cleanup policy",
            "Size on disk",
            "Last write",
            ">orders</a>",
            "Transaction open",
            "read by payments-workers",
            ">payments.settled</a>",
            ">inventory.levels</a>",
            ">customer.profiles</a>",
            "3 segments",
            ">pick-lists</a>",
            ">pick-lists.dlq</a>",
            ">notifications.email</a>",
            ">clickstream</a>",
            "retention 1 h",
            ">orders.dlq</a>",
            "empty: no records yet",
            "9 of 9 topics",
            "One internal topic is hidden.",
            "__consumer_offsets",
            "Why it is read only.",
        ],
    );
    assert!(d.contains("7.9 MiB") && d.contains("128.6 MiB") && d.contains("39.7 MiB"), "sizes");
    assert!(d.contains(">0.2 s ago<") && d.contains(">41 s ago<") && d.contains(">3 d ago<") && d.contains(">never<"));
}

#[test]
fn the_empty_broker_and_the_filter_that_matches_nothing_have_their_own_pages() {
    let none = doc("topics-none");
    in_order(
        "topics-none",
        &none,
        &["<h1>Topics</h1>", "Nothing has been created or written yet.", "This broker holds no topics", "--auto-create-topics", "Or from your app", "kcat -P -b 127.0.0.1:9092 -t orders"],
    );
    let filtered = doc("topics-filtered");
    in_order(
        "topics-filtered",
        &filtered,
        &["Name contains refunds", "Clear filter", "No topic name contains", "refunds", "The filter looks at the 9 topic names", "pick_lists", "Create refunds"],
    );
}

#[test]
fn the_topic_page_has_the_open_transaction_the_toolbar_and_the_ten_newest() {
    let d = doc("topic-detail");
    in_order(
        "topic-detail",
        &d,
        &[
            "<h1 class=\"mono\">orders</h1>",
            "Transaction open",
            "has 3 partitions, keeps records for 7 d",
            "<b>48,213</b>",
            "Partitions",
            "Produce",
            "Delete topic",
            "Messages",
            "Consumers<span class=\"n\">2</span>",
            "Producers<span class=\"n\">3</span>",
            "Transaction orders-eos is open.",
            "Producer 4017 (epoch 2) began it at 22:13:33 UTC on orders-0 and orders-1; the timeout is 1 min.",
            "held 6 records behind",
            "All partitions",
            "Show orders-0 from 16,098",
            "Reading as",
            "read_uncommitted",
            "read_committed",
            "ord-88418",
            "Above LSO",
            "ord-88409",
            "binary · 0a 09 6f 72 64 2d 38 38 …",
            "Newest first · 10 of 48,213",
            "3 of 3 partitions online",
        ],
    );
    assert_eq!(d.matches("<tr><td class=\"mono\">").count(), 10);
}

#[test]
fn the_empty_topic_the_filtered_read_and_the_committed_read_are_their_boards() {
    let empty = doc("topic-messages-empty");
    in_order("topic-messages-empty", &empty, &["orders.dlq", "has 1 partition", "and is empty: offsets 0 to 0, no write yet.", "No records yet", "Produce a record", "Or from your app"]);
    let filtered = doc("topic-messages-filtered");
    in_order(
        "topic-messages-filtered",
        &filtered,
        &["Partition 0", "From offset 16,098", "Clear filters", "The fetch returned", "<b>6</b>", "16,098", "16,103", "the end of the partition", "The call", "Fetch", "Why filter by offset"],
    );
    let committed = doc("topic-readcommitted");
    in_order(
        "topic-readcommitted",
        &committed,
        &[
            "The console reads read_uncommitted by default",
            "7 records are held back from read_committed consumers",
            "orders-0",
            "LSO",
            "16,098",
            "HW",
            "16,104",
            "What each isolation level gets",
            "Who this holds back",
            "orders-eos began at offset 16,098",
            "orders-eos began at offset 16,050",
            "no open transaction",
            "What a read_committed reader sees",
            "ord-88414",
            "ord-88406",
        ],
    );
}

#[test]
fn the_record_pages_show_the_text_and_the_bytes() {
    let r = doc("topic-record");
    in_order(
        "topic-record",
        &r,
        &["orders-1 · 16,050", "One record, completely.", "At or above the last stable offset.", "9 bytes · text", "ord-88412", "86 bytes · JSON", "customer", "Headers", "content-type", "traceparent", "Timestamp", "22:14:17.402", "CreateTime", "Producer", "4017 · epoch 2", "orders-eos", "Ongoing"],
    );
    let b = doc("topic-record-binary");
    in_order(
        "topic-record-binary",
        &b,
        &["orders-2 · 16,056", "The value is not text.", "52 bytes that are not valid UTF-8", "52 bytes · binary", "0000  0a 09 6f 72 64 2d 38 38  ..ord-88", "0030  0a 1c 00 7f              ....", "No headers were sent with this record.", "Position", "below the LSO of orders-2 (16,058)", "Copy as hex"],
    );
}

#[test]
fn the_partitions_and_the_offline_pages_are_their_boards() {
    let p = doc("topic-partitions");
    in_order(
        "topic-partitions",
        &p,
        &["3 partitions, <b>48,213</b> records, 7.9 MiB.", "Add partitions", "One broker.", "Log start", "High watermark", "Last stable offset", "orders-0", "16,104", "16,098", "held 6", "orders-2", "clear", "Where the numbers come from", "ListOffsets"],
    );
    let o = doc("topic-offline");
    in_order(
        "topic-offline",
        &o,
        &["Partition 1 offline", "Partitions 0 and 2 work;", "partition 1</b> went offline after a failed flush", "notifications.email-1 is offline.", "Input/output error (os error 5)", "What works and what does not", "KAFKA_STORAGE_ERROR (56)", "What restarting does", "Restart the machine, not only doze."],
    );
}
