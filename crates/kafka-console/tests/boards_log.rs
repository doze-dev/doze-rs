//! The seven boards of the Log and Producers pages, rendered from the "larder" fixtures with no broker: written to
//! `target/board-diff/<id>.html` for `tools/board-diff.ts`, and checked here against what the boards say (their headings, their
//! labels, their numbers) so that a page that drifts from its board fails before the tool runs.
#![allow(
    clippy::too_many_lines,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::path::PathBuf;

use kafka_console::base::Base;
use kafka_console::boards_log::{IDS, render};

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

/// CON-520: the boards render, whole, and are written for the tool.
#[test]
fn every_board_of_the_slice_renders_and_is_written_for_the_tool() {
    let dir = out_dir();
    std::fs::create_dir_all(&dir).unwrap();
    assert_eq!(IDS.len(), 7);
    for id in IDS {
        let doc = page(id);
        assert!(doc.starts_with("<!DOCTYPE html>"), "{id}");
        std::fs::write(dir.join(format!("{id}.html")), &doc).unwrap();
    }
    assert!(render("topics-list", &Base::root()).is_none());
}

#[test]
fn the_due_log_has_the_parts_of_its_board_in_order() {
    in_order(
        "topic-log",
        &page("topic-log"),
        &[
            "<h1 class=\"mono\">inventory.levels</h1>",
            "Compacted",
            "partition 0 of 1",
            "<b>61,904</b>",
            "offsets in 8 segments, 39.7 MiB.",
            "<b>61 %</b> of the cleanable log is dirty, more than the 50 % it waits for",
            "in <b>15 s</b>",
            "Messages",
            "Producers",
            "The log, offset by offset",
            "log start",
            "checkpoint \u{b7} first dirty offset",
            "<b>39,195</b>",
            "last stable offset = high watermark",
            "<b>61,904</b>",
            "1 \u{b7} 3.2 MiB",
            "kept 41 %",
            "5 \u{b7} 8.0 MiB",
            "cleaned: the latest record of each key",
            "dirty: not cleaned yet",
            "active: never cleaned",
            "hatching: records compaction removed",
            "Width is offsets, not bytes",
            "<b>Gaps.</b>",
            "<b>Log start.</b>",
            "POLICY_VIOLATION (44)",
            "Will the cleaner take it?",
            "Due at the next look",
            "The policy compacts",
            "The dirty share is over the threshold",
            "24.0 MiB dirty of 39.1 MiB cleanable (clean 15.1 MiB)",
            "61 % &gt; 50 %",
            "Nothing is too new to clean",
            "An open transaction below the last stable offset: none today",
            "The active segment is left alone",
            "Looked at every 15 s",
            "A round will rewrite <b>segments 5 to 7</b> (24.0 MiB) in place",
            "move the checkpoint to <b>61,300</b>",
            "Last round",
            "Done",
            "52 % kept",
            "5,120 of 9,775 records stayed: the latest of each key.",
            "<dt>Removed</dt>",
            "4,655",
            "<dt>Took</dt>",
            "1.4 s",
            "<dt>Checkpoint</dt>",
            "<h2>Deletions</h2>",
            "FV-BAN-X",
            "offset 36,402",
            "Read the tombstone",
            "<h2>Durability</h2>",
            "<dt>Vouched for</dt>",
            "<h2>Segments</h2>",
            "Segment 5",
            "holds the checkpoint",
            "00000000000000039195.log",
            "Segment 8",
            "Other logs on this broker",
            "customer.profiles",
            "Why not yet?",
            "Retention",
            "Empty log",
        ],
    );
}

#[test]
fn the_other_three_states_have_the_parts_of_their_boards_in_order() {
    in_order(
        "log-planner",
        &page("log-planner"),
        &[
            "customer.profiles",
            "Not due",
            "<b>31 %</b> of the log is dirty and the cleaner waits for more than 50 %",
            "Not yet: 31 % of the log is dirty, the cleaner waits for more than 50 %.",
            "Why it waits",
            "The dirty share is not over the threshold",
            "31 % \u{2264} 50 %",
            "What would make it due",
            "<dt>Needs</dt>",
            "dirty over 6.1 MiB",
            "<dt>Has</dt>",
            "<dt>Still to come</dt>",
            "min.cleanable.dirty.ratio",
        ],
    );
    in_order(
        "log-retention",
        &page("log-retention"),
        &[
            "<h1 class=\"mono\">orders</h1>",
            "partition 0 of 3",
            "Segments older than <b>7 d</b> are deleted whole, oldest first, every 5 m.",
            "Segment 1 turns 7 d old at <b>22:19</b> and goes at the pass at <b>22:22</b>",
            "from 0 to 5,980.",
            "segment 2 starts: the log start after the pass",
            "high watermark 16,104 \u{b7} last stable offset 16,098",
            "What the next pass will do",
            "Age: 7 d",
            "turns 7 d at 22:19",
            "Size: no limit",
            "Checked every 5 m",
            "After segment 1 goes",
            "<dt>Log start</dt>",
            "0 to 5,980",
            "OFFSET_OUT_OF_RANGE (1)",
            "10,124 offsets",
            "Last pass",
            "Deleted nothing",
            "<dt>Looked at</dt>",
            "orders-0, orders-1, orders-2",
            "<dt>Next pass</dt>",
            "22:17",
        ],
    );
    in_order(
        "log-empty",
        &page("log-empty"),
        &[
            "orders.dlq",
            "Empty",
            "No records yet. A new partition starts with one empty active segment at offset 0",
            "log start = last stable offset = high watermark",
            "1 \u{b7} active segment",
            "0 B, no records",
            "Nothing to compact, nothing to delete",
            "This log has nothing in it",
            "The first record written lands at offset 0.",
            "128.0 MiB",
            "kcat -P -b 127.0.0.1:9092 -t orders.dlq",
            "Produce a record",
        ],
    );
}

#[test]
fn the_producers_and_transaction_pages_have_the_parts_of_their_boards_in_order() {
    in_order(
        "topic-producers",
        &page("topic-producers"),
        &[
            "<h1 class=\"mono\">orders</h1>",
            "Transaction open",
            "partition 0 of 3",
            "<b>3</b> producers are remembered on orders-0.",
            "<b>16,098</b>",
            "held <b>6</b> records behind",
            "orders-0",
            "orders-1",
            "orders-2",
            "Producers on orders-0",
            "Producer id",
            "Last sequence",
            "Seen as",
            "4017",
            "22,918",
            "orders-eos",
            "Hide batches",
            "Producer 4017, the last 5 batches orders-0 remembers",
            "22,915 to 22,918",
            "OUT_OF_ORDER_SEQUENCE_NUMBER (45)",
            "4023",
            "1,140",
            "4031",
            "What the open transaction holds back",
            "Open the transaction",
            "last stable offset",
            "committed records",
            "in the open transaction: 6 records a read_committed consumer cannot see",
            "Aborted transactions remembered: 2",
            "12,877 to 12,880",
            "marker at 12,881",
            "9,310 to 9,315",
        ],
    );
    in_order(
        "txn-open",
        &page("txn-open"),
        &[
            "<b>orders-eos</b> is <b>Ongoing</b>: producer 4017 epoch 2, begun at 22:13:33, <b>41 s</b> into its 1 min timeout.",
            "The transaction",
            "<dt>Transactional id</dt>",
            "<dt>Timeout</dt>",
            "41 s of 1 min",
            "19 s left, then the broker aborts it and fences the producer",
            "<dt>Partitions</dt>",
            "orders-0 from 16,098",
            "orders-1 from 16,050",
            "<dt>Offsets staged</dt>",
            "What it holds back",
            "orders-0: <b>6</b> records, 16,098 to 16,103.",
            "Who is held",
            "If it is not ended",
            "At <b>22:14:33</b>, 19 s from now",
            "the epoch goes from 2 to 3",
            "orders-0, the tail",
            "Only the producer, by ending the transaction",
            "Transactions on this broker",
            "ListTransactions answers by state",
            "PrepareCommit 0",
            "transactional.id.expiration.ms",
        ],
    );
    in_order(
        "txn-holding",
        &page("txn-holding"),
        &[
            "On orders-0 that is <b>16,098</b>, six below the high watermark <b>16,104</b>.",
            "The same, as a consumer sees it",
            "read_uncommitted",
            "<dd class=\"mono\">16,103</dd>",
            "read_committed",
            "<dd class=\"mono\">16,097</dd>",
            "What a Fetch answers on orders-0",
            "high_watermark",
            "16,104",
            "last_stable_offset",
            "ListOffsets latest, read_committed",
            "Which group's fetches are read_committed is a property of each Fetch",
        ],
    );
}
