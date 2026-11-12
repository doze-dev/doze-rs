//! The five boards of slice 9a, rendered from the "larder" fixtures with no broker: written to `target/board-diff/<id>.html`
//! for `tools/board-diff.ts`, and checked here against what the boards say (their headings, their labels, their buttons,
//! their numbers) so that a page that drifts from its board fails before the tool runs.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::path::PathBuf;

use kafka_console::base::Base;
use kafka_console::boards::{IDS, render};

fn out_dir() -> PathBuf {
    // `target/` of the workspace: the directory the tool reads.
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

#[test]
fn every_board_of_the_slice_renders_and_is_written_for_the_tool() {
    let dir = out_dir();
    std::fs::create_dir_all(&dir).unwrap();
    for id in IDS {
        let doc = render(id, &Base::root()).unwrap_or_else(|| panic!("no render for {id}"));
        assert!(doc.starts_with("<!DOCTYPE html>"), "{id}");
        std::fs::write(dir.join(format!("{id}.html")), &doc).unwrap();
    }
    assert!(
        render("topics-list", &Base::root()).is_none(),
        "a board of another slice is not drawn here"
    );
}

#[test]
fn home_has_the_parts_of_its_board_in_its_order() {
    let doc = render("home", &Base::root()).unwrap();
    in_order(
        "home",
        &doc,
        &[
            "<h1>Home</h1>",
            "Running <b>3 d 4 h</b>, last stop clean.",
            "Connect an app",
            "Nothing needs you.",
            "<h2>Address</h2>",
            "Snippets for every client",
            "127.0.0.1:9092",
            "<span class=\"mono\">--listen</span>",
            "Advertised listener",
            "Reachable",
            "Check again",
            "The listener answers",
            "It says where clients should reconnect",
            "That address is reachable",
            "<h2>This instance</h2>",
            "Running",
            "<dt>Started</dt>",
            "<dt>Up for</dt>",
            "<dt>Last stop</dt>",
            "Clean",
            "<dt>Open partitions</dt>",
            "21 of 64",
            "<dt>Descriptors</dt>",
            "<dt>Idle memory</dt>",
            "<dt>Threads</dt>",
            "<dt>Version profile</dt>",
            "Kafka 4",
            "<dt>Access</dt>",
            "No sign-in, no ACLs, plain TCP",
            "Weight and data in Settings",
            "Next: connect something new",
            "kcat -b 127.0.0.1:9092 -X client.id=admin-cli -L",
            "Open Live",
        ],
    );
    assert!(
        doc.contains("data-extra=\"endpoint\"") && doc.contains("data-extra=\"live-sentence\"")
    );
    assert!(doc.contains("data-op=\"Metadata\""));
}

#[test]
fn the_quiet_home_is_the_boards_first_run() {
    let doc = render("home-quiet", &Base::root()).unwrap();
    in_order(
        "home-quiet",
        &doc,
        &[
            "<h1>Your broker is up</h1>",
            "Nothing to look after yet.",
            "<h2>Address</h2>",
            "Advertised listener",
            "Waiting for your first request",
            "Ask the broker what it has",
            "Send a message (auto-create makes the topic, 1 partition)",
            "Watch it arrive",
            "Snippets for your client",
            "Create a topic first",
            "<h2>This instance</h2>",
            "Started 2 min ago on a new data directory.",
            "None yet",
            "0 of 64",
        ],
    );
    assert!(
        !doc.contains("<dt>Up for</dt>") && !doc.contains("<dt>Descriptors</dt>"),
        "the board's quiet card has neither"
    );
}

#[test]
fn the_attention_home_carries_the_boards_transaction() {
    let doc = render("home-attention", &Base::root()).unwrap();
    in_order(
        "home-attention",
        &doc,
        &[
            "callout warn",
            "1 thing needs you: a transaction is close to its timeout.",
            "orders-eos",
            "producer 4017, epoch 2",
            "orders-0",
            "22:13:33 UTC",
            "60 s timeout",
            "Open the transaction",
            "41 s of 60 s",
            "held 6 records behind",
            "last stable offset is 16,098",
            "high watermark 16,104",
        ],
    );
    assert!(
        doc.contains("style=\"--p:68%\""),
        "the bar is at 68 per cent, as the board draws it"
    );
}

#[test]
fn connect_has_the_boards_cards_and_the_five_clients() {
    let doc = render("connect", &Base::root()).unwrap();
    in_order(
        "connect",
        &doc,
        &[
            "<h1>Connect an app</h1>",
            "Copy as client.properties",
            "<h2>Address</h2>",
            "From another machine or a container",
            "Why the advertised host matters",
            "doze-kafka --listen 0.0.0.0:9092",
            "Advertised listener",
            "How the check works, and the bad case",
            "Snippets by client",
            "Copy as",
            ">Code<",
            ">Properties<",
            ">Environment<",
            "<h2 style=\"margin:0\">franz-go</h2>",
            "sarama",
            "librdkafka and kcat",
            "kafkajs",
            "Java client",
            "<h2>Name your app</h2>",
            "No credentials needed.",
            "When SASL is on",
            "security.protocol=SASL_PLAINTEXT",
        ],
    );
    assert_eq!(
        doc.matches("class=\"card\"").count(),
        9,
        "address, advertised, five clients, name your app, SASL"
    );
    assert!(doc.contains("data-extra=\"copy-as\"") && doc.contains("data-extra=\"teaching\""));
}

#[test]
fn the_advertised_page_has_the_check_the_bad_case_and_what_to_set() {
    let doc = render("connect-advertised", &Base::root()).unwrap();
    in_order(
        "connect-advertised",
        &doc,
        &[
            "<h1>Advertised listener</h1>",
            "Check again",
            "<h2>This instance</h2>",
            "Advertised host and port equal the listener",
            "<h2>A bad case, for comparison</h2>",
            "Not reachable",
            "broker.internal:9092",
            "no such host",
            "What your client sees in the bad case",
            "Nothing is wrong with the address you typed.",
            "<h2>The call behind the check</h2>",
            "Metadata v12",
            "then dial 127.0.0.1:9092 and send ApiVersions",
            "<h2>What to set</h2>",
            "From a container, 127.0.0.1 is the container.",
        ],
    );
    assert!(doc.contains("data-extra=\"api-preview\""));
}

#[test]
fn the_fixtures_carry_the_worlds_headline_numbers() {
    // kit/world.md: 9 topics and 21 user partitions, 21 of 64 open, up 3 d 4 h from Nov 6 18:02, idle memory 4.5, 3 threads.
    let v = kafka_console::fixtures::home(kafka_console::model::HomeState::Steady);
    assert_eq!((v.topics, v.partitions), (9, 21));
    assert_eq!(
        (v.instance.open_partitions, v.instance.max_open_partitions),
        (21, 64)
    );
    assert_eq!(v.instance.started, "Nov 6 18:02 UTC");
    assert_eq!(kafka_console::format::span(v.instance.up_secs), "3 d 4 h");
    assert_eq!(v.instance.threads, Some(3));
    assert_eq!(v.instance.open_files, Some(118));
    assert_eq!(
        kafka_console::format::bytes(v.instance.resident_bytes.unwrap()),
        "4.5 MiB"
    );
    assert_eq!(v.listen, "127.0.0.1:9092");
    let a = kafka_console::fixtures::attention();
    assert_eq!(
        (
            a.producer_id,
            a.producer_epoch,
            a.timeout_secs,
            a.elapsed_secs
        ),
        (4017, 2, 60, 41)
    );
    let held = a.held_back.unwrap();
    assert_eq!((held.last_stable, held.high_watermark), (16_098, 16_104));
}
