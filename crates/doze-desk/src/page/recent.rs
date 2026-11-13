//! "Recent · this run": doze's own events for an instance, as the page's two columns (the verb, the rest).

use super::Ink;
use super::parts::Recent;
use super::time::Zone;
use crate::convert;
use doze_control as wire;
use doze_display as words;

/// doze's own events for it, at most the last eight.
pub fn recent(events: &[wire::Event], name: &str, zone: Zone) -> Vec<Recent> {
    let mut out: Vec<Recent> = events
        .iter()
        .filter(|e| e.instance == name)
        .map(|e| {
            let e = convert::event(e);
            let said = words::sentence(&e);
            let (kind, rest) = head(&said);
            Recent {
                at: zone.clock(e.time),
                kind: kind.to_owned(),
                text: rest.to_owned(),
                ink: event_ink(kind),
            }
        })
        .collect();
    let extra = out.len().saturating_sub(8);
    out.drain(..extra);
    out
}

/// The sentence's verb and the rest, for the two columns: a verb of more than one word stays whole ("gave up", not
/// "gave" over "up restarting:", as Go split it), and an event with no detail drops its colon ("crashed", not "crashed:").
fn head(said: &str) -> (&str, &str) {
    let (kind, rest) = ["gave up", "went to sleep", "pinned awake"]
        .into_iter()
        .find_map(|h| said.strip_prefix(h).map(|r| (h, r)))
        .or_else(|| said.split_once(' '))
        .unwrap_or((said, ""));
    (
        kind.trim_end_matches(':'),
        rest.trim().trim_end_matches(':'),
    )
}

fn event_ink(kind: &str) -> Ink {
    match kind {
        "woke" => Ink::Accent,
        "ready" => Ink::Good,
        "idle" | "slept" | "went to sleep" => Ink::Warn,
        "crashed" | "failed" | "gave up" => Ink::Bad,
        _ => Ink::Dim,
    }
}
