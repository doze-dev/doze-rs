//! The words of the Groups pages: the sentence under a group's name, the lag sentences, the list's rebalancing line, Kafka's own
//! descriptions of the codes a group write can answer, and the console's reading of each refusal.
//!
//! The sentences say what the group machine says now and, where the broker's ring of notes has it (slice 9f), why the
//! newest rebalance began; a group the broker has no notes of gives no cause, and nothing is made up.

use crate::format::{ago, number, span_ms};
use crate::lag::Lag;
use crate::model::{
    GroupAttention, GroupHead, GroupPhase, GroupProtocol, GroupSpan as S, LagTopic,
};
use crate::words::list_and;

/// `one`, `two`, ... `ten`, else the digits.
#[must_use]
pub fn number_word(n: usize) -> String {
    const WORDS: [&str; 11] = [
        "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten",
    ];
    WORDS.get(n).map_or_else(
        || number(i64::try_from(n).unwrap_or(i64::MAX)),
        |w| (*w).to_owned(),
    )
}

fn capital(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map_or_else(String::new, |f| f.to_uppercase().chain(c).collect())
}

/// `1 member`, `3 members`.
#[must_use]
pub fn members_word(n: usize) -> String {
    if n == 1 {
        "1 member".to_owned()
    } else {
        format!("{n} members")
    }
}

/// `clickstream (6 partitions) and inventory.levels (1)`.
fn reads(topics: &[&LagTopic]) -> Vec<S> {
    let mut out = Vec::new();
    for (i, t) in topics.iter().enumerate() {
        match (i, topics.len()) {
            (0, _) => {}
            (i, n) if i + 1 == n => out.push(S::text(" and ")),
            _ => out.push(S::text(", ")),
        }
        out.push(S::topic(&t.name));
        let n = t.partitions;
        out.push(S::text(if i == 0 {
            format!(" ({n} {})", if n == 1 { "partition" } else { "partitions" })
        } else {
            format!(" ({n})")
        }));
    }
    out
}

/// The topics a group has something on (committed or held).
fn topics_in_play(topics: &[LagTopic]) -> Vec<&LagTopic> {
    topics
        .iter()
        .filter(|t| !t.holders.is_empty() || t.rows.iter().any(|r| r.committed.is_some()))
        .collect()
}

/// What the lag comes to, as a clause.
fn lag_clause(topics: &[LagTopic]) -> String {
    let committed: Vec<&LagTopic> = topics
        .iter()
        .filter(|t| t.rows.iter().any(|r| r.committed.is_some()))
        .collect();
    if committed.is_empty() {
        return "It has committed nothing yet, so there is no lag to show.".to_owned();
    }
    let ahead = committed
        .iter()
        .flat_map(|t| &t.rows)
        .find(|r| matches!(r.lag, Lag::PastEnd(_)));
    if let Some(r) = ahead
        && let Lag::PastEnd(by) = r.lag
    {
        return format!(
            "Its commit on {}-{} is {} past the end of the log.",
            r.topic,
            r.partition,
            number(by)
        );
    }
    let behind: Vec<&&LagTopic> = committed.iter().filter(|t| t.behind > 0).collect();
    let caught: Vec<String> = committed
        .iter()
        .filter(|t| t.behind == 0)
        .map(|t| t.name.clone())
        .collect();
    match (behind.as_slice(), caught.as_slice()) {
        ([], caught) => format!("It is caught up on {}.", list_and(caught)),
        (behind, []) => format!(
            "It is {}.",
            list_and(
                &behind
                    .iter()
                    .map(|t| format!("{} records behind on {}", number(t.behind), t.name))
                    .collect::<Vec<_>>()
            )
        ),
        (behind, caught) => format!(
            "It is {}; {} {} caught up.",
            list_and(
                &behind
                    .iter()
                    .map(|t| format!("{} records behind on {}", number(t.behind), t.name))
                    .collect::<Vec<_>>()
            ),
            list_and(caught),
            if caught.len() == 1 { "is" } else { "are" }
        ),
    }
}

fn last_commit(h: &GroupHead) -> Option<String> {
    h.last_commit_ms
        .map(|ms| format!("Last commit {}.", ago(h.now_ms - ms)))
}

/// How the group came to be rebalancing, when the group has said: `fulfilment-py-2 left 3 s ago, so the group is rebalancing.`
fn lead_in(h: &GroupHead) -> Option<Vec<S>> {
    let last = h.story.last.as_ref().filter(|l| l.forming)?;
    let (who, clause) = crate::story::last_clause(last);
    let mut out = Vec::new();
    if let Some(w) = who {
        out.push(S::strong(w));
    }
    out.push(S::text(format!("{clause}, so the group is rebalancing.")));
    Some(out)
}

fn rebalancing(h: &GroupHead, topics: &[LagTopic]) -> Vec<S> {
    let mut out = Vec::new();
    if let Some(lead) = lead_in(h) {
        out.extend(lead);
        out.extend(waiting_clause(h));
        return finish(out, topics);
    }
    match &h.rebalance {
        Some(r) if !r.waiting_for.is_empty() => {
            out.push(S::strong(list_and(&r.waiting_for)));
            out.push(S::text(format!(
                " {} not joined again, so the group is rebalancing. It is waiting for {}: {} of the {} rebalance timeout are left.",
                if r.waiting_for.len() == 1 { "has" } else { "have" },
                if r.waiting_for.len() == 1 { "it" } else { "them" },
                span_precise(r.remaining_ms),
                span_precise(r.timeout_ms),
            )));
        }
        _ => out.push(S::text(
            "The group is rebalancing: every member has joined again and the leader is working out who holds what.",
        )),
    }
    finish(out, topics)
}

/// " It is waiting for py-1 to rejoin: 2.1 s of the 5 s rebalance timeout are left."
fn waiting_clause(h: &GroupHead) -> Vec<S> {
    match &h.rebalance {
        Some(r) if !r.waiting_for.is_empty() => vec![
            S::text(" It is waiting for "),
            S::strong(list_and(&r.waiting_for)),
            S::text(format!(
                " to rejoin: {} of the {} rebalance timeout are left.",
                span_precise(r.remaining_ms),
                span_precise(r.timeout_ms),
            )),
        ],
        _ => vec![S::text(
            " Every member has joined again and the leader is working out who holds what.",
        )],
    }
}

/// The topics it reads, at the end of the sentence.
fn finish(mut out: Vec<S>, topics: &[LagTopic]) -> Vec<S> {
    let read = topics_in_play(topics);
    if !read.is_empty() {
        out.push(S::text(" It reads "));
        out.extend(reads(&read));
        out.push(S::text("."));
    }
    out
}

/// `2.1 s`, `45 s`, `3 min`: tenths of a second below a minute.
#[must_use]
pub fn span_precise(ms: u64) -> String {
    if ms < 60_000 {
        if ms % 1000 < 100 {
            return format!("{} s", ms / 1000);
        }
        format!("{}.{} s", ms / 1000, ms % 1000 / 100)
    } else {
        span_ms(i64::try_from(ms).unwrap_or(i64::MAX))
    }
}

fn holding(h: &GroupHead, topics: &[LagTopic]) -> Vec<S> {
    let mut out = Vec::new();
    let n = h.members.len();
    if h.protocol == GroupProtocol::Consumer {
        out.push(S::text(format!(
            "{} {} ",
            capital(&number_word(n)),
            if n == 1 {
                "member holds"
            } else {
                "members hold"
            }
        )));
    } else if n == 1 {
        out.push(S::text("One member, "));
        out.push(S::strong(h.members.first().map_or("", |m| m.id.as_str())));
        out.push(S::text(", reads "));
    } else {
        out.push(S::text(format!(
            "{} members read ",
            capital(&number_word(n))
        )));
    }
    let read = topics_in_play(topics);
    if read.is_empty() {
        out.push(S::text("no topic yet"));
    } else {
        out.extend(reads(&read));
    }
    out.push(S::text("."));
    out
}

/// " The last rebalance was 6 h ago: payments-3c joined." (what the group has said of itself).
fn last_rebalance(h: &GroupHead) -> Vec<S> {
    let Some(last) = h.story.last.as_ref().filter(|l| !l.forming) else {
        return Vec::new();
    };
    let ago = crate::story::ago_near(last.ago_ms);
    let mut out = vec![S::text(format!(" The last rebalance was {ago}: "))];
    let verb = match last.cause {
        crate::model::StoryCause::MemberLeft => "left",
        crate::model::StoryCause::SessionExpired => "timed out",
        crate::model::StoryCause::MemberJoined => "joined",
        _ => "",
    };
    match (&last.who, verb) {
        (Some(w), v) if !v.is_empty() => {
            out.push(S::strong(crate::story::member_label(w)));
            out.push(S::text(format!(" {v}.")));
        }
        _ => out.push(S::text(format!(
            "{}.",
            crate::story::cause_phrase(last.cause)
        ))),
    }
    out
}

/// The sentence under a group's name on its Members page.
#[must_use]
pub fn group_sentence(h: &GroupHead, topics: &[LagTopic]) -> Vec<S> {
    let mut out = match h.phase {
        GroupPhase::Rebalancing if h.protocol == GroupProtocol::Classic => {
            return rebalancing(h, topics);
        }
        GroupPhase::Empty => {
            let mut v = vec![S::mono(&h.id), S::text(" has no members.")];
            if h.commits > 0 {
                v.push(S::text(format!(
                    " It has {} committed {}.",
                    h.commits,
                    if h.commits == 1 { "offset" } else { "offsets" }
                )));
            }
            v
        }
        _ => holding(h, topics),
    };
    if h.protocol == GroupProtocol::Consumer {
        match h.phase {
            GroupPhase::Stable => out.push(S::text(format!(
                " Every member holds exactly what the broker targeted for epoch {}, so nothing is moving.",
                h.generation
            ))),
            GroupPhase::Assigning => out.push(S::text(format!(
                " The group moved to epoch {} and the targets are yet to be computed, at the next heartbeat.",
                h.generation
            ))),
            GroupPhase::Reconciling => {
                let behind = h
                    .members
                    .iter()
                    .filter(|m| m.holds != m.target || m.epoch != h.target_epoch.unwrap_or(m.epoch))
                    .count();
                out.push(S::text(format!(
                    " {} of them {} not yet where the broker wants {}.",
                    capital(&number_word(behind)),
                    if behind == 1 { "is" } else { "are" },
                    if behind == 1 { "it" } else { "them" },
                )));
            }
            _ => {}
        }
    }
    out.push(S::text(format!(" {}", lag_clause(topics))));
    if h.protocol == GroupProtocol::Consumer {
        out.extend(last_rebalance(h));
    }
    if let Some(l) = last_commit(h) {
        out.push(S::text(format!(" {l}")));
    }
    out
}

/// The sentence under the name on the Lag page, by state.
#[must_use]
pub fn lag_sentence(
    h: &GroupHead,
    topics: &[LagTopic],
    state: crate::model::LagState,
    retention_ms: i64,
) -> Vec<S> {
    use crate::model::LagState;
    match state {
        LagState::Ahead => ahead_sentence(h, topics),
        LagState::NoCommits => no_commits_sentence(h, topics),
        LagState::Expiring => expiring_sentence(h, topics, retention_ms),
        LagState::Normal => {
            let mut s = lag_overview(topics);
            s.push_str(&match h.last_commit_ms {
                Some(ms) => format!(" The group committed {}.", ago(h.now_ms - ms)),
                None => String::new(),
            });
            s.push_str(
                " Lag is the log end minus the committed offset, so it counts records the app has not finished with, not records it has not fetched.",
            );
            vec![S::text(s)]
        }
    }
}

fn lag_overview(topics: &[LagTopic]) -> String {
    let committed: Vec<&LagTopic> = topics
        .iter()
        .filter(|t| t.rows.iter().any(|r| r.committed.is_some()))
        .collect();
    let behind: Vec<String> = committed
        .iter()
        .filter(|t| t.behind > 0)
        .map(|t| format!("{} records behind on {}", number(t.behind), t.name))
        .collect();
    let none: Vec<String> = committed
        .iter()
        .filter(|t| t.behind == 0)
        .map(|t| t.name.clone())
        .collect();
    match (behind.is_empty(), none.is_empty()) {
        (true, true) => "Nothing is committed, so nothing can be behind.".to_owned(),
        (true, false) => format!("Caught up on {}.", list_and(&none)),
        (false, true) => format!("{}.", capital(&behind.join(", "))),
        (false, false) => format!(
            "{}, none on {}.",
            capital(&behind.join(", ")),
            list_and(&none)
        ),
    }
}

fn ahead_sentence(h: &GroupHead, topics: &[LagTopic]) -> Vec<S> {
    let first = topics
        .iter()
        .flat_map(|t| &t.rows)
        .find(|r| matches!(r.lag, Lag::PastEnd(_)));
    let Some((r, by)) = first.and_then(|r| match r.lag {
        Lag::PastEnd(by) => Some((r, by)),
        _ => None,
    }) else {
        return vec![S::text("")];
    };
    vec![S::text(format!(
        "{} committed {} on {}-{}, but the log ends at {}: the commit is {} past the end. Lag cannot be a negative number, so the console says where the commit is instead.",
        h.id,
        number(r.committed.unwrap_or(0)),
        r.topic,
        r.partition,
        number(r.end.unwrap_or(0)),
        number(by)
    ))]
}

fn no_commits_sentence(h: &GroupHead, topics: &[LagTopic]) -> Vec<S> {
    let mut s = lag_overview(topics);
    let none: Vec<String> = topics
        .iter()
        .filter(|t| t.rows.iter().all(|r| r.lag == Lag::NoCommit) && !t.rows.is_empty())
        .map(|t| t.name.clone())
        .collect();
    let some: Vec<String> = topics
        .iter()
        .filter(|t| {
            t.rows.iter().any(|r| r.lag == Lag::NoCommit)
                && t.rows.iter().any(|r| r.committed.is_some())
        })
        .map(|t| t.name.clone())
        .collect();
    if !none.is_empty() {
        s.push_str(&format!(
            " {} {} no committed offset, so its lag is not known: the broker has nothing to subtract.",
            list_and(&none),
            if none.len() == 1 { "has" } else { "have" }
        ));
    }
    if !some.is_empty() {
        s.push_str(&format!(
            " Some partitions of {} have no committed offset, so theirs is not known.",
            list_and(&some)
        ));
    }
    if h.phase == GroupPhase::Rebalancing && h.members.iter().all(|m| m.holds.is_empty()) {
        s.push_str(&format!(
            " Nobody holds a partition until generation {} forms.",
            h.generation + 1
        ));
    }
    vec![S::text(s)]
}

fn expiring_sentence(h: &GroupHead, topics: &[LagTopic], retention_ms: i64) -> Vec<S> {
    let first = topics
        .iter()
        .flat_map(|t| &t.rows)
        .filter_map(|r| r.expires_ms)
        .min();
    let n = h.commits;
    let retention = span_ms(retention_ms);
    match first {
        Some(at) => vec![S::text(format!(
            "The broker forgets {}' {} committed {} on {}, in {}. The group has no members, and offsets are kept for {retention}.",
            h.id,
            n,
            if n == 1 { "offset" } else { "offsets" },
            crate::format::date_time(at),
            span_ms(at - h.now_ms),
        ))],
        None => vec![S::text(format!(
            "The broker does not forget {}' committed offsets while things stay as they are: {} Offsets are kept for {retention} once they can be.",
            h.id,
            if h.members.is_empty() {
                "the group may yet have members."
            } else {
                "the group has members, and a group with members keeps what it subscribes to."
            }
        ))],
    }
}

/// The line above the list while a classic group is rebalancing.
#[must_use]
pub fn attention_sentence(a: &GroupAttention) -> Vec<S> {
    let mut v = vec![S::strong(format!("{} is rebalancing.", a.id)), S::text(" ")];
    if let Some(last) = a.last.as_ref().filter(|l| l.forming) {
        let (who, clause) = crate::story::last_clause(last);
        if let Some(w) = who {
            v.push(S::mono(w));
        }
        v.push(S::text(format!("{clause} and the group is waiting for ")));
        v.push(S::mono(if a.waiting_for.is_empty() {
            "the leader".to_owned()
        } else {
            list_and(&a.waiting_for)
        }));
        v.push(S::text(format!(
            " to rejoin, {} of {} left.",
            span_precise(a.remaining_ms),
            span_precise(a.timeout_ms)
        )));
    } else if a.waiting_for.is_empty() {
        v.push(S::text(
            "Every member has joined again and the leader is working out who holds what.",
        ));
    } else {
        v.push(S::text("It is waiting for "));
        v.push(S::mono(list_and(&a.waiting_for)));
        v.push(S::text(format!(
            " to rejoin, {} of {} left.",
            span_precise(a.remaining_ms),
            span_precise(a.timeout_ms)
        )));
    }
    v
}

mod refusals;
pub use refusals::{delete_reading, description, kafka_refusal, reset_reading};

#[cfg(test)]
mod tests;

/// A timeout the way a person says it: `45 s`, `60 s`, `3 min`.
#[must_use]
pub fn seconds(ms: u64) -> String {
    if ms < 120_000 && ms.is_multiple_of(1000) {
        format!("{} s", ms / 1000)
    } else if ms < 60_000 {
        span_precise(ms)
    } else {
        span_ms(i64::try_from(ms).unwrap_or(i64::MAX))
    }
}
