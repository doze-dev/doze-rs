//! The words of the Story pages: what each cause is called and means, how long a generation ran, who held what.

use crate::model::StoryCause;

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
        StoryCause::Forced => "a rebalance was forced from the Lab",
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
