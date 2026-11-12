//! From the fault table's rules to the models the pages draw.

use kafka_server::fault::{Effect, Ended, EndedWhy, FirstHit, MOST_RULES, Rule};

use super::words;
use crate::lab::Lab;
use crate::model::lab::{LabEnd, LabFirst, LabHome, LabRule};
use crate::recorder::{errors, names};

/// How long a rule that is over is kept on the Lab page: an hour. (The table keeps up to 200, however old.)
pub const ENDED_FOR_MS: i64 = 3_600_000;

fn end_of(why: EndedWhy) -> LabEnd {
    match why {
        EndedWhy::Expired => LabEnd::Expired,
        EndedWhy::Spent => LabEnd::Spent,
        EndedWhy::Cleared => LabEnd::Cleared,
        EndedWhy::Replaced => LabEnd::Replaced,
    }
}

fn first_of(f: &FirstHit) -> LabFirst {
    LabFirst {
        at_ms: f.at_ms,
        client: f.client.to_string(),
        api: names::api(f.api).to_owned(),
        version: f.version,
        target: f.target.as_ref().map(|(t, p)| match p {
            Some(p) => format!("{t}-{p}"),
            None => t.to_string(),
        }),
    }
}

/// A rule in force or over, for the pages. `ended`: why it is over, and when.
#[must_use]
pub fn rule_of(r: &Rule, ended: Option<(EndedWhy, i64)>) -> LabRule {
    let spec = &r.spec;
    let (delay_ms, code) = match spec.effect {
        Effect::Slow { delay_ms, .. } => (Some(delay_ms), None),
        Effect::Fail { code } => (None, Some((code, errors::name(code)))),
        Effect::Drop => (None, None),
    };
    let topics = words::topics_words(&spec.matcher);
    let target = match (&spec.matcher.client, topics.is_empty()) {
        (_, false) => topics,
        (Some(c), true) => c.as_str().to_owned(),
        (None, true) => "every request".to_owned(),
    };
    LabRule {
        id: r.id,
        name: r.name.to_string(),
        kind: words::kind_of(spec.effect),
        sentence: words::sentence(spec),
        enabled: r.enabled,
        matched: r.matched,
        hits: r.hits,
        started_ms: r.created_ms,
        ends_ms: ended.map_or(r.ends_ms, |(_, at)| at),
        last_hit_ms: r.last.map(|l| l.at_ms),
        delay_ms,
        code,
        api: words::apis_words(&spec.matcher.apis),
        target,
        share: words::share_words(spec.share),
        first: r.first.as_ref().map(first_of),
        clients: r.clients.iter().map(ToString::to_string).collect(),
        more_clients: r.more_clients,
        ended: ended.map(|(why, _)| end_of(why)),
    }
}

/// An ended rule.
#[must_use]
pub fn ended_of(e: &Ended) -> LabRule {
    rule_of(&e.rule, Some((e.why, e.at_ms)))
}

/// The Lab page from the table and the rehearsals kept: what is in force, what ended in the last hour, what happened once.
#[must_use]
pub fn home(lab: &Lab) -> LabHome {
    let table = lab.table();
    let now_ms = table.now_ms();
    let recent = |at: i64| now_ms.saturating_sub(at) <= ENDED_FOR_MS;
    LabHome {
        off: false,
        now_ms,
        active: table.rules().iter().map(|r| rule_of(r, None)).collect(),
        ended: table
            .ended()
            .iter()
            .filter(|e| recent(e.at_ms))
            .map(ended_of)
            .collect(),
        once: lab.once().into_iter().filter(|o| recent(o.at_ms)).collect(),
        most_rules: MOST_RULES,
        compact_target: None,
    }
}

/// One rule, in force or over, by its number.
#[must_use]
pub fn find(lab: &Lab, id: u32) -> Option<LabRule> {
    let table = lab.table();
    table
        .rule(id)
        .map(|r| rule_of(&r, None))
        .or_else(|| table.ended().iter().find(|e| e.rule.id == id).map(ended_of))
}

#[cfg(test)]
mod tests;
