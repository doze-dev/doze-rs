//! The sentences the pages say, built from view-models. Pure: a table row tests each. The wording is the boards'
//! (reviewed copy); where the broker cannot yet say what a board says, the sentence says what it can, and the place
//! is tagged with the REALITY or GROUNDING reference that explains it.

use crate::format::number;
use crate::model::{HomeState, HomeView, InstanceView, ProbeView, Span, StopView, Verdict};

/// The title of Home: the quiet state has its own (board `home-quiet`).
#[must_use]
pub fn home_title(state: &HomeState) -> &'static str {
    match state {
        HomeState::Quiet => "Your broker is up",
        HomeState::Steady | HomeState::Attention(_) => "Home",
    }
}

/// The sentence under the title: static (CON-805). The numbers it used to hold (how long it has run, how many topics, how many
/// apps spoke, when the last request landed) are the metric slots of Home's strip.
#[must_use]
pub fn home_sub(v: &HomeView) -> Vec<Span> {
    match v.state {
        HomeState::Quiet => vec![Span::text(format!(
            "No topics yet, and that is fine. It is listening on {} and will hold whatever you send it.",
            v.listen
        ))],
        _ => vec![Span::text(
            "What this broker holds, who has spoken to it, and whether anything needs you.",
        )],
    }
}

/// The first line of the instance card: static, the facts are in the rows below it.
#[must_use]
pub fn instance_lede(i: &InstanceView) -> String {
    match i.last_stop {
        StopView::Clean => {
            "The last stop was clean, so every partition was trusted on start.".to_owned()
        }
        StopView::Unclean => {
            "The last stop was not clean, so every partition was checked on start.".to_owned()
        }
        StopView::None => "Started on a new data directory.".to_owned(),
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
pub use topics::{kafka_error, list_and, read_refused, retention, timeout};

#[cfg(test)]
mod tests;
