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
            "reach back to 19:12, 3 h 2 min.",
            "Pause",
            "Filter by client, API or topic",
            "All apps",
            "All topics",
            "Refused only",
            "Idle polls hidden",
            "Clients · 9",
            "Refused among the rows kept: 3.",
            "INVALID_RECORD (87)",
            "MESSAGE_TOO_LARGE (10)",
            "INVALID_PRODUCER_EPOCH (47)",
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
            "Showing 16 of 2,000 matching rows",
            "Older",
        ],
    );
    assert_eq!(d.matches("data-row=").count(), 16);
    assert!(d.contains("aria-live=\"polite\""));
    assert!(d.contains("data-extra=\"live-sentence\""));
}

#[test]
fn the_three_states_say_what_the_boards_say() {
    let paused = doc("live-paused");
    in_order(
        "live-paused",
        &paused,
        &[
            "Paused at 22:14:23.6. Everything below is as it was then.",
            "Resume",
            "Paused. ",
            "17",
            " requests are held back.",
            "The wire, as of 22:14:23.6",
            "The last 7 requests before the pause.",
            "17 newer",
        ],
    );
    assert_eq!(paused.matches("data-row=").count(), 7);
    let quiet = doc("live-quiet");
    in_order(
        "live-quiet",
        &quiet,
        &[
            "Nothing has reached the broker since it started at 22:12.",
            "127.0.0.1:9092",
            "Send the first request",
            "kcat -P -b 127.0.0.1:9092",
            "Snippets for your client",
            "No requests yet",
        ],
    );
    assert!(!quiet.contains("Clients ·"));
    let load = doc("live-load");
    in_order(
        "live-load",
        &load,
        &[
            "A busy minute: 571 requests",
            "Repeats fold into one row.",
            "× 412",
            "The wire, folded",
            "7 rows stand for 571 requests.",
            "about 36 records each",
            "median",
        ],
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
