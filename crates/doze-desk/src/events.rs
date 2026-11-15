//! The journal a stack keeps: the last events, oldest first, each once.

use doze_control::Event;

/// How many journal events a stack keeps, as the Go dash and hub did.
pub const EVENTS_KEPT: usize = 300;

/// Adds `e` to `events` unless it is already there, keeping the last [`EVENTS_KEPT`]. Returns whether it was added.
pub(crate) fn add(events: &mut Vec<Event>, e: &Event) -> bool {
    if seen(events, e) {
        return false;
    }
    events.push(e.clone());
    if events.len() > EVENTS_KEPT {
        let extra = events.len() - EVENTS_KEPT;
        events.drain(..extra);
    }
    true
}

/// Whether the journal already holds `e`: a daemon replays its journal on a resubscribe from the start (or a
/// scripted one with no sequence numbers does), and a page must not read an event twice. Numbered events are the
/// same when their numbers are; others when their time, kind, instance and detail are.
fn seen(events: &[Event], e: &Event) -> bool {
    events.iter().rev().any(|h| {
        if e.seq > 0 && h.seq > 0 {
            return h.seq == e.seq;
        }
        h.time.unix_nanos() == e.time.unix_nanos()
            && h.kind == e.kind
            && h.instance == e.instance
            && h.detail == e.detail
    })
}
