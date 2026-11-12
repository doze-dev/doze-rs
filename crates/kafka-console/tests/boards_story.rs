//! The three boards of slice 9f rendered from the "larder" notes with no broker: written to `target/board-diff/<id>.html` for
//! `tools/board-diff.ts`, and checked against what the boards say (the generations, the causes, the rule).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::path::PathBuf;

use kafka_console::base::Base;
use kafka_console::boards::{STORY_IDS, render_story};

fn out_dir() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest.parent().unwrap().parent().unwrap();
    let target =
        std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| root.join("target"), PathBuf::from);
    target.join("board-diff")
}

fn doc(id: &str) -> String {
    render_story(id, &Base::root()).unwrap_or_else(|| panic!("no render for {id}"))
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
    for id in STORY_IDS {
        let d = doc(id);
        assert!(d.starts_with("<!DOCTYPE html>"), "{id}");
        std::fs::write(dir.join(format!("{id}.html")), &d).unwrap();
    }
    assert!(render_story("home", &Base::root()).is_none());
}

#[test]
fn the_story_has_the_boards_four_generations_newest_first_and_its_cards() {
    in_order(
        "group-story",
        &doc("group-story"),
        &[
            "<span class=\"k\">Rebalances kept</span>",
            "<span class=\"k\">Newest</span>",
            "Forming",
            "Story<span class=\"n\">4</span>",
            ">Now</h2>",
            "Generation 31 began at ",
            "22:14:19.4",
            "Rejoin deadline",
            "Generations, newest first",
            "Member left",
            "Generation 31 is forming",
            "sent LeaveGroup. Members 2 to 1.",
            "Not rejoined: ",
            "2.1 s left of 5 s.",
            "Open this rebalance",
            "Member joined",
            "Generation 30",
            "Members 1 to 2. Settled in 1.1 s.",
            "fulfilment-py-1 held payments.settled-0, -1 and pick-lists-0; fulfilment-py-2 held payments.settled-2 and -3. Lasted 5 m 17 s.",
            "Session expired",
            "Generation 29",
            "sent no heartbeat for 10 s, its session timeout. Members 2 to 1. Settled in 0.3 s.",
            "Leader rejoined",
            "Generation 28",
            "200 events per group",
            "emptied",
            "Where the cause comes from",
            "The causes it can name",
            "Is it flapping?",
            "Not by the rule: one session expiry",
            "Check the rule",
            "What a calm group looks like",
            "search-indexer",
            "joined an empty group and led it. Members 0 to 1. Settled in 0.2 s.",
            "It holds clickstream-0 to 5 and inventory.levels-0. Lasted 3 h 44 m so far.",
        ],
    );
}

#[test]
fn the_rebalance_drawer_says_who_when_the_deadline_and_what_moves() {
    in_order(
        "story-rebalance",
        &doc("story-rebalance"),
        &[
            "Generation 31",
            "Member left",
            "Waiting, 2.1 s of 5 s left",
            ">Who<",
            "fulfilment-py-2",
            "LeaveGroup, sent no reason",
            ">Began<",
            "22:14:19.411",
            ">Deadline<",
            "22:14:24.411",
            "the longest rebalance timeout among members, 5 s",
            ">Not rejoined<",
            "fulfilment-py-1",
            ">Members<",
            "2 before, 1 after",
            ">What moves<",
            "payments.settled-2 and -3",
            "held by fulfilment-py-2",
            ">If nobody rejoins<",
            "The group goes Empty. Offsets stay.",
            "See the LeaveGroup on the wire",
        ],
    );
    let d = doc("story-rebalance");
    assert!(d.contains("role=\"dialog\""), "a drawer is a dialog");
    assert!(d.contains("data-overlay"), "over the Story page");
}

#[test]
fn the_flapping_page_holds_the_rule_against_the_group_and_draws_the_pattern() {
    in_order(
        "story-flapping",
        &doc("story-flapping"),
        &[
            "Back to the story",
            "A group is flapping when the same member keeps timing out and coming back.",
            "is <b>not</b> flapping",
            "The rule, against fulfilment",
            "Three or more expired-then-joined cycles in 10 minutes.",
            "Expired, then joined, 3 times in 10 minutes",
            "0 cycles",
            "A leave is not a timeout",
            "not counted",
            "Session expiries in the last",
            "fulfilment-py-3 at 19:44:51 only.",
            "A member joined and left within 6 minutes",
            "5 m 17 s",
            "What it looks like when it is",
            "Illustration, not your group",
            "Where to look.",
            "session.timeout.ms",
        ],
    );
}

#[test]
fn a_group_with_one_rebalance_and_nothing_since_has_the_quiet_story() {
    let v = kafka_console::fixtures_story::story("search-indexer").unwrap();
    let page = kafka_console::pages::story::story_page(&v, &Base::root()).into_string();
    in_order(
        "quiet",
        &page,
        &[
            "See its lag",
            "<span class=\"k\">Rebalances kept</span>",
            "Story<span class=\"n\">1</span>",
            "Member joined",
            "Generation 8",
            "It holds clickstream-0 to 5 and inventory.levels-0. Lasted 3 h 44 m so far.",
            "A quiet story is a good one",
            "No member has come, gone or timed out since 18:30:12.",
            "So the lag on this group is not caused by rebalancing",
        ],
    );
    assert!(
        !page.contains(">Now</h2>") && !page.contains("Where the cause comes from"),
        "no cards to read"
    );
}
