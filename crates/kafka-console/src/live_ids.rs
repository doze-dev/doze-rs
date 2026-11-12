//! The fragment ids of the Live pages and their docks: the part of the stream's closed vocabulary that slice 9e adds.
//! `stream::parse_watch` accepts these as well as its own.

/// The Live tape: the sentence, the refusals callout, the table and its pager.
pub const LIVE_WIRE: &str = "live-wire";
/// The "On the wire" dock of a topic page.
pub const TOPIC_WIRE: &str = "topic-wire";
/// The wire page of a group.
pub const GROUP_WIRE: &str = "group-wire";

/// What the stream may be asked to watch for these pages.
pub const WATCHABLE: &[&str] = &[LIVE_WIRE, TOPIC_WIRE, GROUP_WIRE];

/// What the Live page watches.
pub const LIVE_IDS: &[&str] = &[LIVE_WIRE];
/// What a topic's wire page watches.
pub const TOPIC_WIRE_IDS: &[&str] = &[TOPIC_WIRE];
/// What a group's wire page watches.
pub const GROUP_WIRE_IDS: &[&str] = &[GROUP_WIRE];
