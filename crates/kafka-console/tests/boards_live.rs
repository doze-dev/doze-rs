//! The twelve boards of slice 9e rendered from the "larder" fixtures with no broker: written to `target/board-diff/<id>.html`
//! for `tools/board-diff.ts`, and checked here against what the boards say so that a page that drifts from its board fails
//! before the tool runs.
#![allow(
    clippy::too_many_lines,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::path::PathBuf;

use kafka_console::base::Base;
use kafka_console::boards::{LIVE_IDS_BOARDS, render_live};

fn out_dir() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest.parent().unwrap().parent().unwrap();
    let target =
        std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| root.join("target"), PathBuf::from);
    target.join("board-diff")
}

fn doc(id: &str) -> String {
    render_live(id, &Base::root()).unwrap_or_else(|| panic!("no render for {id}"))
}

/// The drawer of a board: what follows the opening of its panel.
fn drawer(id: &str) -> String {
    let d = doc(id);
    let at = d
        .find("class=\"drawer-panel\"")
        .unwrap_or_else(|| panic!("{id}: no drawer"));
    d[at..].to_owned()
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
fn every_board_of_the_slice_renders_and_is_written_for_the_tool() {
    let dir = out_dir();
    std::fs::create_dir_all(&dir).unwrap();
    for id in LIVE_IDS_BOARDS {
        let d = doc(id);
        assert!(d.starts_with("<!DOCTYPE html>"), "{id}");
        std::fs::write(dir.join(format!("{id}.html")), &d).unwrap();
    }
    assert!(render_live("home", &Base::root()).is_none());
}

#[test]
fn live_wire_has_the_boards_sentence_filters_refusals_and_sixteen_rows() {
    let d = doc("live-wire");
    in_order(
        "live-wire",
        &d,
        &[
            "<h1>Live</h1>",
            "Everything your apps send, newest first.",
            "Reaches back to",
            "19:12",
            "Clients",
            "Refusals",
            "INVALID_RECORD (87)",
            "MESSAGE_TOO_LARGE (10)",
            "INVALID_PRODUCER_EPOCH (47)",
            "Filter by client, API or topic",
            "All apps",
            "All topics",
            "Refused only",
            "Idle polls hidden",
            ">Time</th>",
            ">Client</th>",
            ">API</th>",
            ">Target</th>",
            ">Result</th>",
            ">Took</th>",
            "22:14:22.904",
            "payments.settled-1",
            "from 7,402",
            "1 batch, 211 B",
            "Showing ",
            ">16</span> of <span",
            " matching rows",
            "Older",
        ],
    );
    assert_eq!(d.matches("data-row=").count(), 16);
    // The page holds no number in a sentence (CON-805), no explainer box above the table, and a tail that is offered, not on.
    assert!(!d.contains("aria-live=\"polite\"") || d.contains("data-conn"));
    assert!(d.contains("data-tail-url=") && d.contains("data-tail-mode=\"live\""));
    assert!(!d.contains("Repeats fold into one row, a line") && !d.contains("data-stream"));
    assert!(d.contains("class=\"stats\"") && d.contains("data-frag=\"/fragments/live-stats\""));
}

#[test]
fn the_page_is_the_same_page_in_every_state_and_the_states_are_slots() {
    // CON-807: a state never moves a block. The quiet wire, the busy wire and the steady one have the same parts in the same order.
    let parts = [
        "<h1>Live</h1>",
        "class=\"stats\"",
        "among the rows kept",
        "class=\"toolbar\"",
        "class=\"tailbar\"",
        "<table class=\"t dt-t\"",
        "<div class=\"pager\"",
    ];
    for id in ["live-wire", "live-quiet", "live-load"] {
        let d = doc(id);
        in_order(id, &d, &parts);
        assert!(
            !d.contains("data-stream") && !d.contains("data-pause"),
            "{id}"
        );
    }
    let quiet = doc("live-quiet");
    assert!(quiet.contains("No requests yet. Send one with any Kafka client, for example: echo hi | kcat -P -b 127.0.0.1:9092"));
    assert_eq!(quiet.matches("data-row=").count(), 0);
    assert!(quiet.contains("No refusals among the rows kept."));
    let load = doc("live-load");
    assert!(
        load.contains("× 412"),
        "a folded row says how many it stands for"
    );
}

#[test]
fn the_drawers_have_their_boards_parts() {
    let d = drawer("request-detail");
    in_order(
        "request-detail",
        &d,
        &[
            "Fetch v12",
            "Done",
            "fulfilment",
            "Client",
            "Asked for",
            "payments.settled",
            "from offset 7,402",
            "Answer",
            "1 batch, 211 B",
            "error_code 0 (NONE)",
            "Took",
            "Correlation id",
            "9,114",
            "What the broker kept.",
            "Request bodies and record bytes are not recorded",
            "Copy as",
            "kcat -C -b 127.0.0.1:9092 -t payments.settled",
            "kafka-console-consumer.sh",
            "Open payments.settled",
            "Fetch it again",
        ],
    );
    assert!(d.contains("data-op=\"Fetch\""));
    let r = drawer("request-refused");
    in_order(
        "request-refused",
        &r,
        &[
            "INVALID_RECORD (87)",
            "Refused",
            "Kafka's words",
            "Compacted topic cleanup.policy=compact requires every record to have a key",
            "The rule",
            "What would pass",
            "Written",
            "Nothing",
            "Produce with a key",
            "Config",
            "The log",
        ],
    );
    assert!(
        r.contains("kcat -P -K:"),
        "the keyed example for a compacted topic"
    );
    let f = drawer("request-fenced");
    in_order(
        "request-fenced",
        &f,
        &[
            "INVALID_PRODUCER_EPOCH (47)",
            "Producer attempted to produce with an old epoch.",
            "How it came to this",
            "InitProducerId",
            "The request's epoch is current",
            "What would pass",
            "InitProducerId",
            "How fencing works",
        ],
    );
    let t = drawer("request-too-large");
    in_order(
        "request-too-large",
        &t,
        &[
            "MESSAGE_TOO_LARGE (10)",
            "The request included a message larger than the max message size",
            "1,203,441",
            "The rule",
            "Batch size is within the limit",
            "1,048,588",
            "Where the limit comes from",
            "broker",
            "What would pass",
            "Raise max.message.bytes",
            "Open orders",
        ],
    );
    for id in [
        "request-detail",
        "request-refused",
        "request-fenced",
        "request-too-large",
        "live-clients",
    ] {
        let d = doc(id);
        assert!(
            d.contains("role=\"dialog\"") && d.contains("aria-labelledby"),
            "{id}"
        );
    }
}

#[test]
fn the_clients_the_first_request_and_the_docks() {
    let c = drawer("live-clients");
    in_order(
        "live-clients",
        &c,
        &[
            "Clients",
            "9 clients connected, by",
            "payments",
            "127.0.0.1:50388 + 3 more",
            "franz-go",
            "Produce v9",
            "admin-cli",
            "Only clients that are connected now.",
        ],
    );
    let f = doc("connect-first-request");
    in_order(
        "connect-first-request",
        &f,
        &[
            "A new client just connected",
            "admin-cli",
            "sent its first request at",
            "Open Live",
            "It works.",
            "Who it is",
            "client.id",
            "Software",
            "What it asked, in order",
            "ApiVersions v3",
            "Metadata v12",
            "Waiting for its next request",
            "The wire for this client",
        ],
    );
    let t = doc("topic-wire");
    in_order(
        "topic-wire",
        &t,
        &[
            "On the wire: orders",
            "Client requests that touched this topic",
            "Collapse to the dock",
            "Topic and partition",
            "orders-0",
            "Kept in memory and emptied at every restart.",
            "The console's own requests are not listed",
        ],
    );
    let g = doc("group-wire");
    in_order(
        "group-wire",
        &g,
        &[
            "fulfilment",
            "Every client request that touched fulfilment",
            "Back to the group",
            ">Request</th>",
            ">What it touched</th>",
            "× 71",
            "ILLEGAL_GENERATION (22)",
            "Specified generation id is not valid.",
        ],
    );
}
