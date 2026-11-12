//! The words of the Story pages: what each cause is called and means, how long a generation ran, who held what.

use crate::model::StoryCause;

/// The chip on a generation: `Member left`.
#[must_use]
pub const fn cause_title(c: StoryCause) -> &'static str {
    match c {
        StoryCause::MemberJoined => "Member joined",
        StoryCause::MemberLeft => "Member left",
        StoryCause::SessionExpired => "Session expired",
        StoryCause::TopicChanged => "Topic changed",
        StoryCause::ProtocolsChanged => "Rejoined with new protocols",
        StoryCause::LeaderRejoined => "Leader rejoined",
        StoryCause::RebalanceTimeout => "Rebalance timeout",
        StoryCause::SubscriptionChanged => "Subscription changed",
        StoryCause::RightsChanged => "Rights changed",
        StoryCause::BecameClassic => "Became classic",
        StoryCause::BecameConsumer => "Became a consumer group",
    }
}

/// The cause in a sentence: `a member timed out`.
#[must_use]
pub const fn cause_phrase(c: StoryCause) -> &'static str {
    match c {
        StoryCause::MemberJoined => "a member joined",
        StoryCause::MemberLeft => "a member left",
        StoryCause::SessionExpired => "a member timed out",
        StoryCause::TopicChanged => "a topic changed",
        StoryCause::ProtocolsChanged => "a member rejoined with other protocols",
        StoryCause::LeaderRejoined => "the leader rejoined",
        StoryCause::RebalanceTimeout => "members did not rejoin in time",
        StoryCause::SubscriptionChanged => "a member changed what it subscribes to",
        StoryCause::RightsChanged => "a right to read a topic changed",
        StoryCause::BecameClassic => "the group became a classic group again",
        StoryCause::BecameConsumer => "the group became a consumer group",
    }
}

/// What the cause means, for the card that lists them.
#[must_use]
pub const fn cause_help(c: StoryCause) -> &'static str {
    match c {
        StoryCause::MemberJoined => "a new member, or the first of an empty group",
        StoryCause::MemberLeft => "LeaveGroup, sent by the client",
        StoryCause::SessionExpired => "no heartbeat within the session timeout",
        StoryCause::TopicChanged => {
            "partitions added to a subscribed topic, or one made or deleted (consumer-protocol groups)"
        }
        StoryCause::ProtocolsChanged => "a member changed what it offers",
        StoryCause::LeaderRejoined => {
            "the leader of a running group joined again; clients do it when a topic they read changes"
        }
        StoryCause::RebalanceTimeout => {
            "members did not rejoin in time, or give up what they were told to"
        }
        StoryCause::SubscriptionChanged => {
            "a consumer-protocol member asked for other topics, a pattern or an assignor"
        }
        StoryCause::RightsChanged => {
            "a right to describe a topic was given to a member's principal or taken from it"
        }
        StoryCause::BecameClassic => {
            "the last consumer-protocol member left a group that still had classic members"
        }
        StoryCause::BecameConsumer => {
            "a consumer-protocol member joined a classic group of consumers"
        }
    }
}

/// How long a generation ran, the way the board writes it: `1.1 s`, `5 m 17 s`, `2 h 24 m`, `3 d 4 h`.
#[must_use]
pub fn lasted(ms: u64) -> String {
    let s = ms / 1000;
    if ms < 60_000 {
        return crate::groups_words::span_precise(ms);
    }
    let (m, h, d) = (s / 60, s / 3600, s / 86_400);
    if s < 3600 {
        format!("{m} m {} s", s % 60)
    } else if s < 86_400 {
        format!("{h} h {} m", m % 60)
    } else {
        format!("{d} d {} h", h % 24)
    }
}

/// A member's id as a person reads it: the client's name, without the unique tail the broker adds to a member id
/// (`fulfilment-py-1-7b0f6a0e-...`: a client id and a UUID). The id as it is stays in the page's `title`.
#[must_use]
pub fn member_label(id: &str) -> String {
    let Some(cut) = id.len().checked_sub(37) else {
        return id.to_owned();
    };
    match id.get(cut..) {
        Some(tail) if tail.starts_with('-') && is_uuid(&tail[1..]) && cut > 0 => {
            id[..cut].to_owned()
        }
        _ => id.to_owned(),
    }
}

fn is_uuid(s: &str) -> bool {
    let groups: Vec<&str> = s.split('-').collect();
    groups.len() == 5
        && groups
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(g, n)| g.len() == n && g.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// `payments.settled-0, -1 and pick-lists-0`: the partitions of each topic, the first as `topic-N`, the next of the same topic
/// as `-N`, a run of three or more as `topic-0 to 5`.
#[must_use]
pub fn parts_phrase(topics: &[(String, Vec<i32>)]) -> String {
    let mut items: Vec<String> = Vec::new();
    for (topic, partitions) in topics {
        let mut ps = partitions.clone();
        ps.sort_unstable();
        ps.dedup();
        let mut first = true;
        for (lo, hi) in runs(&ps) {
            if hi - lo >= 2 {
                let lead = if first {
                    format!("{topic}-")
                } else {
                    "-".to_owned()
                };
                items.push(format!("{lead}{lo} to {hi}"));
                first = false;
            } else {
                for p in lo..=hi {
                    items.push(if first {
                        format!("{topic}-{p}")
                    } else {
                        format!("-{p}")
                    });
                    first = false;
                }
            }
        }
    }
    crate::words::list_and(&items)
}

/// Runs of consecutive numbers in a sorted list: `[0, 1, 2, 5]` is `[(0, 2), (5, 5)]`.
fn runs(sorted: &[i32]) -> Vec<(i32, i32)> {
    let mut out: Vec<(i32, i32)> = Vec::new();
    for &p in sorted {
        match out.last_mut() {
            Some((_, hi)) if *hi + 1 == p => *hi = p,
            _ => out.push((p, p)),
        }
    }
    out
}

/// What a member held: `py-1 held payments.settled-0, -1 and pick-lists-0`; `present` says it in the present tense, and
/// `alone` (the only member) as `It holds ...`.
#[must_use]
pub fn holds_phrase(
    member: &str,
    topics: &[(String, Vec<i32>)],
    present: bool,
    alone: bool,
) -> String {
    let who = if alone && present {
        "It".to_owned()
    } else {
        member_label(member)
    };
    let verb = if present { "holds" } else { "held" };
    if topics.is_empty() {
        format!("{who} {verb} nothing")
    } else {
        format!("{who} {verb} {}", parts_phrase(topics))
    }
}

/// How long ago, to the nearest second under a minute (`3 s ago` for 2.9 s), else as the pages say it.
#[must_use]
pub fn ago_near(ms: u64) -> String {
    if (1000..60_000).contains(&ms) {
        format!("{} s ago", (ms + 500) / 1000)
    } else {
        crate::format::ago(i64::try_from(ms).unwrap_or(i64::MAX))
    }
}

/// The newest rebalance as a clause that follows its age: `fulfilment-py-2 left 3 s ago`, `a topic changed 6 h ago`. The
/// member is returned apart, so a page can set it in bold.
#[must_use]
pub fn last_clause(last: &crate::model::StoryLast) -> (Option<String>, String) {
    let ago = ago_near(last.ago_ms);
    let who = last.who.as_deref().map(member_label);
    let verb = match last.cause {
        StoryCause::MemberLeft => Some("left"),
        StoryCause::SessionExpired => Some("timed out"),
        StoryCause::MemberJoined => Some("joined"),
        _ => None,
    };
    match (who, verb) {
        (Some(w), Some(v)) => (Some(w), format!(" {v} {ago}")),
        _ => {
            let phrase = cause_phrase(last.cause);
            let mut c = phrase.chars();
            let head: String = c
                .next()
                .map(|f| f.to_uppercase().collect())
                .unwrap_or_default();
            (None, format!("{head}{} {ago}", c.as_str()))
        }
    }
}
