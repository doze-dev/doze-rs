//! The four boards of the Settings page, rendered from the "larder" fixtures with no broker: written to
//! `target/board-diff/<id>.html` for `tools/board-diff.ts`, and checked here against what the boards say (their headings, their
//! labels, their buttons, their numbers) so that a page that drifts from its board fails before the tool runs.
#![allow(
    clippy::too_many_lines,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::path::PathBuf;

use kafka_console::base::Base;
use kafka_console::boards_settings::{IDS, render};

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

/// CON-550: the boards render, whole, and are written for the tool.
#[test]
fn every_board_of_the_slice_renders_and_is_written_for_the_tool() {
    let dir = out_dir();
    std::fs::create_dir_all(&dir).unwrap();
    assert_eq!(IDS.len(), 4);
    for id in IDS {
        let doc = page(id);
        assert!(doc.starts_with("<!DOCTYPE html>"), "{id}");
        std::fs::write(dir.join(format!("{id}.html")), &doc).unwrap();
    }
    assert!(
        render("topics-list", &Base::root()).is_none(),
        "a board of another family is not drawn here"
    );
}

/// CON-551: the Settings page has the sections of its board in order, with the board's numbers.
#[test]
fn the_settings_page_has_the_parts_of_its_board_in_order() {
    in_order(
        "settings-home",
        &page("settings-home"),
        &[
            "<h1>Settings</h1>",
            "Five settings can change while it runs",
            "Broker configs",
            "Version profile",
            "Intervals",
            "Weight",
            "Data",
            "Flags",
            "<b>0</b> of the 5 are set at run time",
            "Running <b>3 d 4 h</b>",
            "<h2>Broker configs</h2>",
            "log.segment.bytes",
            "134217728",
            "128 MiB",
            "Static broker",
            "flag --segment-bytes; Kafka's default is 1 GiB",
            "At the next append of each open partition",
            "message.max.bytes",
            "1 MiB + 12 B",
            "On the next request",
            "log.retention.ms",
            "At the next retention pass, every 5 m",
            "in Kafka's words",
            "<h2>Version profile</h2>",
            "<b>38 APIs</b>",
            "Produce",
            "v3 to v9",
            "Produce and fetch",
            "Show all 38 APIs",
            "SaslHandshake (v1) and SaslAuthenticate (v0 to v2)",
            "<h2>Intervals</h2>",
            "Retention check",
            "--log-retention-check-interval",
            "Cleaner look",
            "Flush after a write",
            "Offsets retention",
            "First rebalance wait",
            "Consumer session",
            "<h2>Weight</h2>",
            "Idle memory",
            "4.5<small>MB</small>",
            "Open partitions",
            "21<small>of 64</small>",
            "File descriptors",
            "118",
            "Binary",
            "Allowed open",
            "<h2>Data</h2>",
            "The data directory",
            "~/larder/data/kafka",
            "Copy path",
            "187.0 MiB",
            "What a restart does",
            "<h2>Flags</h2>",
            "<b>1</b> flag differs from its default",
            "Show all 37 flags",
            "Network and storage",
            "Topics and partitions",
            "Cleaning, retention and groups",
            "Profile and process",
            "Security: sign-in, ACLs and TLS",
            "Console",
            "Refused at start, with exit code 2.",
            "What is on, in Access",
        ],
    );
}

/// CON-552: the edit dialog is the board's, over the Settings page.
#[test]
fn the_edit_dialog_is_the_boards_over_its_parent() {
    let doc = page("broker-config-edit");
    in_order(
        "broker-config-edit",
        &doc,
        &[
            "<h1>Settings</h1>",
            "role=\"dialog\"",
            "Edit log.retention.ms",
            "How long a topic's segments are kept before retention may delete them.",
            "Config",
            "Applies to",
            "All brokers",
            "This broker (1)",
            "New value",
            "259200000",
            "ms = 3 d",
            "Now:",
            "Allowed:",
            "Takes effect:",
            "Who sees it:",
            "payments.settled",
            "The call",
            "IncrementalAlterConfigs",
            "Cancel",
            "Check",
            "Save",
        ],
    );
    assert!(doc.contains("data-op=\"IncrementalAlterConfigs\""));
    assert!(doc.contains("data-extra=\"teaching\"") && doc.contains("data-extra=\"api-preview\""));
}

/// CON-553: the saved and refused states say what the board says.
#[test]
fn the_saved_and_refused_states_carry_their_boards_words() {
    in_order(
        "broker-config-saved",
        &page("broker-config-saved"),
        &[
            "<h1>Settings</h1>",
            "<b>Saved.</b>",
            "log.retention.ms",
            "259,200,000",
            "(3 d)",
            "for all brokers.",
            "<h2>Dynamic broker configs</h2>",
            "1 of the 5 is now set at run time",
            "Dynamic default broker",
            "set for all brokers",
            "<h2>Who sees it</h2>",
            "4 topics have no retention.ms of their own and now follow",
            "The call that was made",
            "-&gt; 200, no error",
        ],
    );
    in_order(
        "broker-config-refused",
        &page("broker-config-refused"),
        &[
            "<h1>Refused</h1>",
            "Back to Settings",
            "INVALID_REQUEST (42)",
            "Cannot update these configs dynamically: [num.partitions]",
            "The rule: only five broker configs act on a running broker.",
            "<h2>What would pass</h2>",
            "--default-partitions",
            "All flags",
            "<h2>The other ways a change is refused</h2>",
            "Unknown Dynamic Configuration: foo.bar.",
            "Value must be at least 1048576",
            "Unexpected broker id, expected 1, but received 2",
            "The call that was refused",
            "-&gt; INVALID_REQUEST (42)",
            "<h2>Fixed while it runs</h2>",
            "num.partitions",
            "node.id",
        ],
    );
}
