//! The words of the Lab: pure functions from a rule to the sentences the pages say about it. Nothing here reads a clock or a counter,
//! so a sentence is made once from the rule and does not change while the rule runs.

use std::time::Duration;

use kafka_server::fault::{AddError, Effect, Matcher, Share, Spec};
use kafka_wire::messages::ApiKey;

use crate::model::lab::LabKind;
use crate::recorder::{errors, names};

/// The error codes the Fail form offers, with the words under each (the board's three).
pub const CODES: [(i16, &str); 3] = [
    (
        7,
        "The broker took too long. Clients retry; most treat it as a normal hiccup.",
    ),
    (
        6,
        "This broker is not the partition's leader. A client refreshes its metadata and asks again.",
    ),
    (
        3,
        "The topic looks missing. Some clients stop; some create it if they may.",
    ),
];

/// How often a rule fires, as the list in the form writes it: its value and its words.
pub const SHARES: [(&str, &str); 10] = [
    ("every", "Every request"),
    ("2", "1 in 2"),
    ("3", "1 in 3"),
    ("4", "1 in 4"),
    ("5", "1 in 5"),
    ("10", "1 in 10"),
    ("20", "1 in 20"),
    ("p10", "A random 10 %"),
    ("p50", "A random 50 %"),
    ("p90", "A random 90 %"),
];

/// How long a rule lasts, as the form's choices write it: minutes and words.
pub const ENDS: [(u64, &str); 5] = [
    (1, "1 min"),
    (5, "5 min"),
    (10, "10 min"),
    (30, "30 min"),
    (60, "1 hour"),
];

/// The share a form value names.
#[must_use]
pub fn share_of(value: &str) -> Option<Share> {
    match value {
        "every" => Some(Share::Every),
        v => {
            if let Some(p) = v.strip_prefix('p') {
                return p
                    .parse()
                    .ok()
                    .filter(|p| (1..=99).contains(p))
                    .map(Share::Percent);
            }
            v.parse()
                .ok()
                .filter(|n| (2..=1000).contains(n))
                .map(Share::EveryNth)
        }
    }
}

/// `1 in 4`, `every request`, `a random 10 %`: how often, as the tables write it.
#[must_use]
pub fn share_words(s: Share) -> String {
    match s {
        Share::Every => "every request".to_owned(),
        Share::EveryNth(n) => format!("1 in {n}"),
        Share::Percent(p) => format!("a random {p} %"),
    }
}

/// `Produce`, `Produce and Fetch`, `any`.
#[must_use]
pub fn apis_words(apis: &[ApiKey]) -> String {
    let names: Vec<&str> = apis.iter().map(|a| names::api(*a)).collect();
    match names.as_slice() {
        [] => "any".to_owned(),
        [one] => (*one).to_owned(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

/// What the matcher names, as a phrase after "to": `orders`, `orders-1`, `orders and payments`; empty when it names no topic.
#[must_use]
pub fn topics_words(m: &Matcher) -> String {
    let topics: Vec<&str> = m.topics.iter().map(|t| t.as_str()).collect();
    let base = match topics.as_slice() {
        [] => String::new(),
        [one] => (*one).to_owned(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    };
    match (m.partitions.as_slice(), topics.len()) {
        ([], _) | (_, 0) => base,
        ([p], 1) => format!("{base}-{p}"),
        (ps, _) => {
            let list: Vec<String> = ps.iter().map(ToString::to_string).collect();
            format!("{base}, partitions {}", list.join(", "))
        }
    }
}

fn from_words(m: &Matcher) -> String {
    m.client
        .as_ref()
        .map_or_else(String::new, |c| format!(" from {}", c.as_str()))
}

fn requests(spec: &Spec) -> String {
    let apis = apis_words(&spec.matcher.apis);
    let what = if spec.matcher.apis.is_empty() {
        "request".to_owned()
    } else {
        format!("{apis} request")
    };
    match spec.share {
        Share::Every => format!("every {what}"),
        Share::EveryNth(n) => format!("1 in {n} {what}s"),
        Share::Percent(p) => format!("a random {p} % of {what}s"),
    }
}

/// What a rule does, in a sentence: `+400 ms on 1 in 4 Produce requests to orders.` It holds no number that follows the broker.
#[must_use]
pub fn sentence(spec: &Spec) -> String {
    let to = topics_words(&spec.matcher);
    let to = if to.is_empty() {
        String::new()
    } else {
        format!(" to {to}")
    };
    let from = from_words(&spec.matcher);
    match spec.effect {
        Effect::Slow {
            delay_ms,
            jitter_ms,
        } => {
            let jitter = if jitter_ms > 0 {
                format!(" and up to {jitter_ms} ms more")
            } else {
                String::new()
            };
            format!("+{delay_ms} ms{jitter} on {}{to}{from}.", requests(spec))
        }
        Effect::Fail { code } => format!(
            "Answers {}{to}{from} with {}.",
            requests(spec),
            errors::name(code)
        ),
        Effect::Drop => {
            let who = spec
                .matcher
                .client
                .as_ref()
                .map_or("every app", |c| c.as_str());
            if spec.matcher.up_to_conn.is_some() {
                format!("Closes the connections {who} has open now, at their next request.")
            } else {
                format!("Closes each connection from {who} at its first request.")
            }
        }
    }
}

/// What the rule is called when its maker gave it no name: `Slow produce to orders`, `Fail inventory.levels fetches`.
#[must_use]
pub fn default_name(spec: &Spec) -> String {
    let api = spec
        .matcher
        .apis
        .first()
        .map(|a| names::api(*a).to_lowercase());
    let topic = topics_words(&spec.matcher);
    let on = |verb: &str, plural: bool| match (&api, topic.is_empty()) {
        (Some(a), false) if plural => format!("{verb} {topic} {}", plural_of(a)),
        (Some(a), false) => format!("{verb} {a} to {topic}"),
        (Some(a), true) => format!("{verb} {a}"),
        (None, false) => format!("{verb} requests to {topic}"),
        (None, true) => format!("{verb} requests"),
    };
    match spec.effect {
        Effect::Slow { .. } => on("Slow", false),
        Effect::Fail { .. } => on("Fail", true),
        Effect::Drop => match &spec.matcher.client {
            Some(c) if spec.matcher.up_to_conn.is_some() => {
                format!("Drop {} once", c.as_str())
            }
            Some(c) => format!("Keep dropping {}", c.as_str()),
            None => "Drop connections".to_owned(),
        },
    }
}

fn plural_of(word: &str) -> String {
    if word.ends_with("ch") || word.ends_with('s') {
        format!("{word}es")
    } else {
        format!("{word}s")
    }
}

/// `10 min`, `1 hour`, `45 s`.
#[must_use]
pub fn ttl_words(ttl: Duration) -> String {
    let s = ttl.as_secs();
    if s < 60 {
        format!("{s} s")
    } else if s < 3600 {
        format!("{} min", s / 60)
    } else if s.is_multiple_of(3600) {
        format!("{} hour{}", s / 3600, if s == 3600 { "" } else { "s" })
    } else {
        format!("{} h {} min", s / 3600, s % 3600 / 60)
    }
}

/// The kind of a rule.
#[must_use]
pub fn kind_of(e: Effect) -> LabKind {
    match e {
        Effect::Slow { .. } => LabKind::Slow,
        Effect::Fail { .. } => LabKind::Fail,
        Effect::Drop => LabKind::Drop,
    }
}

/// What the engine refused, in a sentence.
#[must_use]
pub fn refused_words(e: AddError) -> String {
    match e {
        AddError::TooMany => {
            "The Lab holds 50 rules at once. Clear one, or wait for one to end.".to_owned()
        }
        AddError::BadName => "The name is too long or holds a control character.".to_owned(),
        AddError::BadDuration => "A rule lasts from 1 second to 24 hours.".to_owned(),
        AddError::BadDelay => "A delay is from 1 ms to 60,000 ms.".to_owned(),
        AddError::BadShare => "That share of requests is not one the Lab offers.".to_owned(),
        AddError::BadCode => "Kafka has no such error code.".to_owned(),
        AddError::FailNeedsApi => "A rule that fails requests names the API it fails.".to_owned(),
        AddError::CannotFail(api) => format!(
            "The Lab cannot make up an answer for {}: it would not be one error code.",
            names::api(api)
        ),
        AddError::BadPartition => {
            "A partition is a whole number from 0, at most 64 of them.".to_owned()
        }
        AddError::TooManyTopics => "A rule names at most 16 topics.".to_owned(),
        AddError::BadHits => {
            "A rule that stops after a number of hits needs at least 1.".to_owned()
        }
    }
}

/// What the wire shows for a request a rule touched: the words for the preview.
#[must_use]
pub fn wire_words(spec: &Spec) -> String {
    let api = spec
        .matcher
        .apis
        .first()
        .map_or("Request", |a| names::api(*a));
    match spec.effect {
        Effect::Slow { delay_ms, .. } => {
            format!("{api}, tagged lab, answered after its usual time and {delay_ms} ms more")
        }
        Effect::Fail { code } => {
            format!("{api}, tagged lab, Failed {} ({code})", errors::name(code))
        }
        Effect::Drop => "The connection is closed with no answer, tagged lab".to_owned(),
    }
}

#[cfg(test)]
mod tests;
