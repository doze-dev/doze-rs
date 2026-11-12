//! The sentences the pages say, built from view-models. Pure: a table row tests each. The wording is the boards'
//! (reviewed copy); where the broker cannot yet say what a board says, the sentence says what it can, and the place
//! is tagged with the REALITY or GROUNDING reference that explains it.

use crate::format::{count, number, span};
use crate::model::{HomeState, HomeView, InstanceView, ProbeView, Span, StopView, Verdict};

/// The title of Home: the quiet state has its own (board `home-quiet`).
#[must_use]
pub fn home_title(state: &HomeState) -> &'static str {
    match state {
        HomeState::Quiet => "Your broker is up",
        HomeState::Steady | HomeState::Attention(_) => "Home",
    }
}

/// The sentence under the title. The boards add "N apps appear on the wire ... the last request landed 1 s ago": that is
/// the recorder's (GROUNDING 2.3, slice 9e), so here the broker says what it holds instead, never a count of apps.
#[must_use]
pub fn home_sub(v: &HomeView) -> Vec<Span> {
    let holds = || {
        format!(
            "It holds {} and {}.",
            count(v.topics, "topic", "topics"),
            count(v.partitions, "partition", "partitions")
        )
    };
    match v.state {
        HomeState::Quiet => vec![Span::text(format!(
            "No topics yet, and that is fine. It is listening on {} and will hold whatever you send it.",
            v.listen
        ))],
        _ => vec![
            Span::text("Running "),
            Span::strong(span(v.instance.up_secs)),
            Span::text(format!(
                "{} {}{}",
                last_stop_short(v.instance.last_stop),
                holds(),
                spoke_sentence(v.spoke.as_ref())
            )),
        ],
    }
}

/// What the recorder adds to Home's sentence: how many clients the wire knows, how far back it reaches, how long ago the last
/// request landed. Nothing when this console has no recorder; a plain "no client has spoken" for an empty tape.
fn spoke_sentence(s: Option<&crate::model::SpokeView>) -> String {
    let Some(s) = s else { return String::new() };
    match (s.apps, s.oldest_ms, s.ago_ms) {
        (0, _, _) => " No client has spoken yet.".to_owned(),
        (n, Some(oldest), Some(ago)) => format!(
            " {} appear on the wire, which reaches back to {}, and the last request landed {}.",
            count(n, "app", "apps"),
            live::hhmm(oldest),
            crate::format::ago(ago)
        ),
        (n, _, _) => format!(" {} appear on the wire.", count(n, "app", "apps")),
    }
}

/// What follows the running time: the board's ", last stop clean." and its two other cases.
fn last_stop_short(stop: StopView) -> &'static str {
    match stop {
        StopView::Clean => ", last stop clean.",
        StopView::Unclean => ", last stop not clean.",
        StopView::None => " on a new data directory.",
    }
}

/// The first line of the instance card.
#[must_use]
pub fn instance_lede(i: &InstanceView) -> String {
    let up = span(i.up_secs);
    match i.last_stop {
        StopView::Clean => {
            format!("Up {up}. The last stop was clean, so every partition was trusted on start.")
        }
        StopView::Unclean => format!(
            "Up {up}. The last stop was not clean, so every partition was checked on start."
        ),
        StopView::None => format!("Started {up} ago on a new data directory."),
    }
}

/// The two words of the "Open partitions" row: `21 of 64`, or the count alone where there is no bound.
#[must_use]
pub fn open_partitions(i: &InstanceView) -> String {
    let open = i64::try_from(i.open_partitions).unwrap_or(i64::MAX);
    match i.max_open_partitions {
        0 => number(open),
        max => format!(
            "{} of {}",
            number(open),
            number(i64::try_from(max).unwrap_or(i64::MAX))
        ),
    }
}

/// The lede of the advertised-listener card on Home.
#[must_use]
pub fn home_advertised_lede(p: &ProbeView) -> String {
    let head = "A client asks for Metadata and then reconnects to the address in the answer, not to the one you gave it.";
    if p.same {
        format!(
            "{head} Here that is {}, the address the listener binds.",
            p.advertised
        )
    } else {
        format!(
            "{head} Here that is {}, not the address the listener binds ({}).",
            p.advertised, p.listen
        )
    }
}

/// The lede of the advertised-listener card on Connect.
#[must_use]
pub fn connect_advertised_lede(p: &ProbeView) -> Vec<Span> {
    let where_ = if p.same {
        format!("{}, which is the address it listens on", p.advertised)
    } else {
        format!(
            "{}, not the address it listens on ({})",
            p.advertised, p.listen
        )
    };
    let answers = match p.verdict {
        Verdict::Reachable => ", and answers there.",
        Verdict::NotReachable => ", and does not answer there.",
        Verdict::NotChecked => ".",
    };
    vec![
        Span::text("After Metadata a client reconnects to the "),
        Span::strong("advertised"),
        Span::text(format!(
            " address. This broker advertises {where_}{answers}"
        )),
    ]
}

/// The lede of the "This instance" card of the advertised-listener page.
#[must_use]
pub fn advertised_instance_lede(p: &ProbeView) -> String {
    match (p.same, p.verdict) {
        (_, Verdict::NotChecked) => {
            "The check did not finish in time: nothing is claimed. Check again.".to_owned()
        }
        // Only what the check did: it dialled that address and it answered (the board adds "so a client that reached it once
        // can always reach it again", which a check made once, from this machine, cannot promise).
        (true, Verdict::Reachable) => {
            "Advertised host and port equal the listener, and the check reached that address."
                .to_owned()
        }
        (true, Verdict::NotReachable) => {
            "Advertised host and port equal the listener, yet a rule below failed.".to_owned()
        }
        // (The board says "a client that reached {listen}": that is claimed only when the check reached it, and the rules
        // below say whether it did.)
        (false, _) => format!(
            "Advertised host and port differ from the listener: a client is told to go to {}, not to {}.",
            p.advertised, p.listen
        ),
    }
}

/// The word beside a card title for a verdict.
#[must_use]
pub fn verdict_word(v: Verdict) -> &'static str {
    match v {
        Verdict::Reachable => "Reachable",
        Verdict::NotReachable => "Not reachable",
        Verdict::NotChecked => "Not checked",
    }
}

/// The words of the callout when something needs someone (the one condition the boards draw). With `unchecked` open
/// transactions not looked at, the count is a floor and says so.
#[must_use]
pub fn attention_headline(count: usize, unchecked: usize) -> String {
    match (count, unchecked) {
        (1, 0) => "1 thing needs you: a transaction is close to its timeout.".to_owned(),
        (_, 0) => {
            format!("{count} things need you: {count} transactions are close to their timeout.")
        }
        (1, _) => "At least 1 transaction is close to its timeout.".to_owned(),
        (_, _) => format!("At least {count} transactions are close to their timeout."),
    }
}

/// What Home says of the open transactions it did not look at (none: nothing to say).
#[must_use]
pub fn unchecked_note(unchecked: usize) -> Option<String> {
    match unchecked {
        0 => None,
        1 => Some("1 open transaction was not checked.".to_owned()),
        n => Some(format!("{n} open transactions were not checked.")),
    }
}

pub mod live;
mod reach;
mod topics;

pub use reach::reach_sentence;
pub use topics::{
    kafka_error, list_and, list_sentence, partitions_sentence, read_refused, retention, timeout,
    topic_sentence,
};

#[cfg(test)]
mod tests;
