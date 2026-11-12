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
                "{} {}",
                last_stop_short(v.instance.last_stop),
                holds()
            )),
        ],
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
        (_, Verdict::NotChecked) => "The check did not finish in time: nothing is claimed. Check again.".to_owned(),
        (true, Verdict::Reachable) => "Advertised host and port equal the listener, so a client that reached it once can always reach it again.".to_owned(),
        (true, Verdict::NotReachable) => "Advertised host and port equal the listener, yet a rule below failed.".to_owned(),
        (false, _) => format!(
            "Advertised host and port differ from the listener: a client that reached {} is told to go to {}.",
            p.listen, p.advertised
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

/// The words of the callout when something needs someone (the one condition the boards draw).
#[must_use]
pub fn attention_headline(count: usize) -> String {
    if count == 1 {
        "1 thing needs you: a transaction is close to its timeout.".to_owned()
    } else {
        format!("{count} things need you: {count} transactions are close to their timeout.")
    }
}

mod topics;

pub use topics::{list_and, list_sentence, partitions_sentence, reach_sentence, retention, topic_sentence};

#[cfg(test)]
mod tests;
