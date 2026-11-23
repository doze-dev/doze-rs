//! EventBridge replays: the list, a replay's page, and Start replay.

use super::super::Format::{Json, Mono, Tail, Text as Plain, Time};
use super::super::{Id, Kind, List, Step, Text, col, field, ops, or, state};

pub(super) const REPLAYS: Kind = Kind {
    id: "replays",
    title: "Replays",
    one: "replay",
    about: "Archived events sent back to the bus they came from.",
    members: &["ReplayName"],
    list: List {
        step: Step::new("ListReplays", r#"{"Limit":100,"NextToken":"{token}"}"#),
        items: "Replays",
        item_as: "ReplayName",
        token_out: "NextToken",
        id: "ReplayName",
        name: Text::at("ReplayName"),
        label: None,
        split: None,
    },
    elsewhere: "",
    each: &[],
    columns: &[
        col("Source", "EventSourceArn", Tail),
        state("State", "State", Plain),
        col("Event window start", "EventStartTime", Time),
        col("Event window end", "EventEndTime", Time),
        col("Last event replayed", "EventLastReplayedTime", Time),
        col("Started", "ReplayStartTime", Time),
    ],
    id_of: Id::Name,
    detail: &[Step::new("DescribeReplay", r#"{"ReplayName":"{id}"}"#)],
    fields: &[
        field("ARN", "ReplayArn", Mono),
        field("Description", "Description", Plain),
        field("State", "State", super::STATES),
        or("State reason", "StateReason", Plain, "None"),
        field("Source archive", "EventSourceArn", Mono),
        field("Destination", "Destination", Json),
        field("Event window start", "EventStartTime", Time),
        field("Event window end", "EventEndTime", Time),
        field("Last event replayed", "EventLastReplayedTime", Time),
        field("Started", "ReplayStartTime", Time),
        field("Ended", "ReplayEndTime", Time),
    ],
    subs: &[],
    create: None,
    delete: None,
    reveal: None,
    deep: &[],
    // Start replay (a page of its own) and what it reads: the archives, the buses and their rules.
    ops: &[
        ops::free("StartReplay"),
        ops::free("CancelReplay"),
        ops::free("ListReplays"),
        ops::free("ListArchives"),
        ops::free("ListRules"),
    ],
    ..Kind::BARE
};
