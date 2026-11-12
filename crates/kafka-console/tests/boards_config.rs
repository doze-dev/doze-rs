//! The eight boards of the Config pages, rendered from the "larder" fixtures with no broker: written to
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
use kafka_console::boards_config::{IDS, render};

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

/// CON-250: the boards render, whole, and are written for the tool.
#[test]
fn every_board_of_the_slice_renders_and_is_written_for_the_tool() {
    let dir = out_dir();
    std::fs::create_dir_all(&dir).unwrap();
    assert_eq!(IDS.len(), 8);
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

#[test]
fn the_config_page_has_the_parts_of_its_board_in_order() {
    in_order(
        "topic-config",
        &page("topic-config"),
        &[
            "<h1 class=\"mono\">inventory.levels</h1>",
            "Compacted",
            "<b>33</b> configs. <b>3</b> are set on this topic, <b>1</b> comes from a broker flag, <b>29</b> are Kafka's defaults.",
            "within 15 s",
            "Messages",
            "Config",
            "Consumers",
            "Producers",
            "Filter configs",
            "Changed",
            "Compaction",
            "Segments",
            "Where a value comes from, first match wins:",
            "<h2>Changed from Kafka's default <span class=\"pill\">4</span></h2>",
            "Set here or by a flag.",
            "cleanup.policy",
            "keep the latest record of each key",
            "Dynamic topic",
            "segment.bytes",
            "8 MiB; the flag is 128 MiB",
            "Where segment.bytes comes from.",
            "Static flag",
            "Reset the topic value and the flag, 134217728, takes over.",
            "retention.ms",
            "no time limit",
            "min.compaction.lag.ms",
            "<h2>Cleanup and compaction <span class=\"pill\">3</span></h2>",
            "min.cleanable.dirty.ratio",
            "Kafka's text for min.cleanable.dirty.ratio",
            "<dt>Type</dt>",
            "<dt>Allowed</dt>",
            "<dt>Default</dt>",
            "<dt>Here</dt>",
            "<h2>Retention <span class=\"pill\">1</span></h2>",
            "<h2>Segments and index <span class=\"pill\">6</span></h2>",
            "<h2>Records <span class=\"pill\">8</span></h2>",
            "<h2>Durability <span class=\"pill\">2</span></h2>",
            "<h2>Replication and tiered storage <span class=\"pill\">9</span></h2>",
            "9 configs Kafka lists that this broker has no use for",
        ],
    );
}

#[test]
fn every_row_says_when_a_change_counts_and_the_ones_the_broker_ignores_say_so() {
    let doc = page("topic-config");
    for text in [
        "A change counts on the next request.",
        "A change counts at the cleaner's next look, within 15 s.",
        "A change counts at the next retention pass, within 5 m.",
        "A change counts from the next append, by the open partition's writer; existing segments keep their size.",
        "This broker keeps the value and shows it; it does nothing with it.",
    ] {
        assert!(doc.contains(text), "{text}");
    }
}

#[test]
fn the_edit_dialogs_have_the_parts_of_their_boards() {
    in_order(
        "config-edit",
        &page("config-edit"),
        &[
            "<h2 id=\"config-edit-title\">Edit min.cleanable.dirty.ratio</h2>",
            "New value for",
            "value=\"0.7\"",
            "<b>What it is</b> how much of the log may be dirty before the cleaner takes it",
            "<b>Now</b>",
            "Kafka's default",
            "<b>Allowed</b> a number from 0 to 1",
            "<b>Takes effect</b> at the cleaner's next look, within 15 s",
            "The call",
            "IncrementalAlterConfigs",
            "min.cleanable.dirty.ratio  SET  0.7",
            "Cancel",
            "Check",
            "Save",
        ],
    );
    in_order(
        "config-edit-list",
        &page("config-edit-list"),
        &[
            "Edit cleanup.policy",
            "It is a list. What to do with it?",
            "Append",
            "Subtract",
            "Set",
            "Policy to append",
            "value=\"compacted\"",
            "<b>Allowed</b> compact or delete, each element checked",
            "cleanup.policy  APPEND  compacted",
            "Save",
        ],
    );
    in_order(
        "config-edit-lag",
        &page("config-edit-lag"),
        &[
            "Edit compaction lag",
            "min.compaction.lag.ms",
            "value=\"7200000\"",
            "2 h",
            "max.compaction.lag.ms",
            "value=\"3600000\"",
            "1 h",
            "<b>Allowed</b> min",
            "min.compaction.lag.ms  SET  7200000",
            "max.compaction.lag.ms  SET  3600000",
        ],
    );
}

#[test]
fn the_result_states_have_the_parts_of_their_boards() {
    in_order(
        "config-saved",
        &page("config-saved"),
        &[
            "<b>min.cleanable.dirty.ratio</b> is now <b>0.7</b>, set on this topic. The other 32 configs are unchanged",
            "Back to the config",
            "<b>Saved.</b> IncrementalAlterConfigs answered <span class=\"mono\">0</span> for <span class=\"mono\">inventory.levels</span>.",
            "<h2>Before and after</h2>",
            "<th scope=\"col\">Before</th>",
            "0.5",
            "Default",
            "0.7",
            "Dynamic topic",
            "<h2>What it does to this log</h2>",
            "A change counts at the cleaner's next look, within 15 s.",
        ],
    );
    in_order(
        "config-refused",
        &page("config-refused"),
        &[
            "Nothing was changed. Kafka checks the two lags together",
            "Refused",
            "INVALID_CONFIG (40)",
            "conflict topic config setting min.compaction.lag.ms (7200000) &gt; max.compaction.lag.ms (3600000)",
            "<h2>What you sent</h2>",
            "Both keys were in one request, so both were refused.",
            "<h2>What would pass</h2>",
            "<dt>min</dt>",
            "3600000",
            "<dt>max</dt>",
            "7200000",
            "Edit again",
        ],
    );
    in_order(
        "config-refused-value",
        &page("config-refused-value"),
        &[
            "INVALID_CONFIG (40)",
            "Invalid value compacted for configuration cleanup.policy: String must be one of: compact, delete",
            "<h2>What you sent</h2>",
            "cleanup.policy  APPEND  compacted",
            "<h2>What would pass</h2>",
            "-&gt; compact,delete",
            "<h2>The same words for other ranges</h2>",
            "segment.bytes 512",
            "Value must be at least 1024",
            "min.cleanable.dirty.ratio 1.5",
            "Value must be no more than 1",
            "segment.bytes abc",
            "Not a number of type INT",
        ],
    );
}

#[test]
fn the_reset_dialog_has_the_parts_of_its_board() {
    in_order(
        "config-reset",
        &page("config-reset"),
        &[
            "Reset segment.bytes to default?",
            "is removed, so the topic follows the broker again.",
            "<dt>Now</dt>",
            "8388608",
            "8 MiB",
            "Dynamic topic",
            "<dt>After</dt>",
            "134217728",
            "128 MiB",
            "Static flag",
            "<b>Takes effect</b> from the next append",
            "segment.bytes  DELETE",
            "Reset to default",
        ],
    );
}
