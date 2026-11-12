//! The sixteen boards of slice 9c rendered from the "larder" group fixtures with no broker: written to
//! `target/board-diff/<id>.html` for `tools/board-diff.ts`, and checked against what the boards say (headings, names, numbers).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::path::PathBuf;

use kafka_console::base::Base;
use kafka_console::boards::{GROUPS_IDS, render_groups};

fn out_dir() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest.parent().unwrap().parent().unwrap();
    let target =
        std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| root.join("target"), PathBuf::from);
    target.join("board-diff")
}

fn doc(id: &str) -> String {
    render_groups(id, &Base::root()).unwrap_or_else(|| panic!("no render for {id}"))
}

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
fn every_board_of_the_slice_renders_and_is_written_for_the_tool() {
    let dir = out_dir();
    std::fs::create_dir_all(&dir).unwrap();
    for id in GROUPS_IDS {
        let d = doc(id);
        assert!(d.starts_with("<!DOCTYPE html>"), "{id}");
        std::fs::write(dir.join(format!("{id}.html")), &d).unwrap();
    }
    assert!(render_groups("home", &Base::root()).is_none());
}

#[test]
fn the_list_has_the_boards_four_groups_in_order_with_their_lag_words() {
    in_order(
        "groups-list",
        &doc("groups-list"),
        &[
            "<h1>Groups</h1>",
            "fulfilment is rebalancing.",
            "waiting for <span class=\"mono\">fulfilment-py-1</span> to rejoin, 2.1 s of 5 s left.",
            ">analytics</a>",
            "offsets expire in 3 d 2 h",
            "Past the end",
            "orders-0 is 96 beyond its log",
            ">fulfilment</a>",
            "generation 31 is forming",
            "pick-lists: no commits yet",
            ">payments-workers</a>",
            "consumer (KIP-848)",
            "12 in each partition",
            ">search-indexer</a>",
            "classic, roundrobin",
            "18,240",
            "4 of 4 groups",
        ],
    );
}

#[test]
fn the_group_pages_say_what_the_boards_say() {
    in_order(
        "group-detail",
        &doc("group-detail"),
        &[
            "search-indexer",
            "classic",
            "roundrobin",
            "generation 8",
            "One member, <b>search-indexer-1</b>, reads",
            "clickstream</a> (6 partitions) and",
            "It is 18,240 records behind on clickstream",
            "Session timeout",
            "45 s",
            "Rebalance timeout",
            "60 s",
        ],
    );
}

#[test]
fn the_consumer_group_shows_target_and_assigned_and_the_rebalance_shows_who_is_waited_for() {
    in_order(
        "group-detail-848",
        &doc("group-detail-848"),
        &[
            "consumer protocol (KIP-848)",
            "epoch 14",
            "Group epoch",
            "Targets computed for",
            "Assignor",
            "uniform",
            "Session timeout",
            "payments-3a",
            "orders-0",
            "In step",
            "12 in each",
        ],
    );
    in_order(
        "group-rebalancing",
        &doc("group-rebalancing"),
        &[
            "generation 31 forming",
            "<b>fulfilment-py-1</b> has not joined again",
            "Waiting for",
            "2.1",
            "What happens next",
            "Not rejoined",
            "payments.settled-0, payments.settled-1, pick-lists-0",
        ],
    );
}

#[test]
fn the_lag_pages_never_show_a_negative_number_and_say_where_a_commit_is() {
    let ahead = doc("lag-ahead");
    in_order(
        "lag-ahead",
        &ahead,
        &[
            "analytics committed 16,200 on orders-0, but the log ends at 16,104",
            "96 past the end",
            "Kafka accepted this commit.",
        ],
    );
    assert!(!ahead.contains(">-"), "a negative number");
    in_order(
        "lag-no-commits",
        &doc("lag-no-commits"),
        &[
            "21 records behind on payments.settled",
            "pick-lists has no committed offset",
            "No commits yet",
            "No commit is not lag 2,310.",
        ],
    );
    in_order(
        "lag-expiring",
        &doc("lag-expiring"),
        &[
            "Retention window",
            "7 d",
            "--offsets-retention 168h",
            "Forgotten at",
            "Nov 13 00:14",
        ],
    );
}

#[test]
fn the_refusals_carry_kafkas_code_and_words() {
    in_order(
        "group-reset-refused",
        &doc("group-reset-refused"),
        &[
            "Refused",
            "UNKNOWN_MEMBER_ID (25)",
            "The coordinator is not aware of this member.",
            "What would pass",
            "answer: UNKNOWN_MEMBER_ID (25)",
        ],
    );
    in_order(
        "group-delete-refused",
        &doc("group-delete-refused"),
        &[
            "NON_EMPTY_GROUP (68)",
            "The group is not empty.",
            "answer: NON_EMPTY_GROUP (68)",
        ],
    );
}
