//! The writes of the Groups pages: the reset of offsets (with the preview it applies), the delete of a group and the delete
//! of a group's offsets on a topic. Each is a protocol request as the console's principal; a refusal is Kafka's own code and
//! words. The preview and the write are the same computation, so what the page showed is what is sent.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use kafka_wire::api::Response;
use kafka_wire::messages::ApiKey;

use super::group::group;
use super::group_read as read;
use crate::backend::{Backend, requests};
use crate::calls_groups as calls;
use crate::format::parse_time;
use crate::groups_words::{delete_reading, kafka_refusal, reset_reading};
use crate::lag::lag;
use crate::model::{
    GroupDeleteView, GroupHead, GroupLagView, GroupRefused, OffsetDeleteView, ResetMode, ResetRow,
    ResetView,
};

/// What a click on Reset came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResetResult {
    /// Moved this many partitions.
    Done(usize),
    /// Kafka said no.
    Refused(Box<GroupRefused>),
    /// The choice cannot be sent (no topic, a value that is not a number): the dialog again.
    Invalid(Box<ResetView>),
    NoGroup,
}

/// What a delete of a group came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeleteGroupResult {
    Done,
    Refused(Box<GroupRefused>),
    NoGroup,
}

/// What a delete of offsets came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OffsetDeleteResult {
    /// Forgot this many commits.
    Done(usize),
    /// Kafka said no: the dialog again, with its words.
    Refused(Box<OffsetDeleteView>, crate::model::Refusal),
    /// No topic was chosen.
    Nothing(Box<OffsetDeleteView>),
    NoGroup,
}

/// The mode a radio and a typed value mean, or why not.
///
/// # Errors
/// A sentence for the page when the value is not what the mode needs.
pub fn parse_mode(to: &str, value: &str) -> Result<ResetMode, String> {
    let value = value.trim();
    let number = |what: &str| {
        value
            .parse::<i64>()
            .map_err(|_| format!("{what} must be a whole number."))
    };
    match to {
        "earliest" => Ok(ResetMode::Earliest),
        "offset" => number("The offset").and_then(|n| {
            if n < 0 {
                Err("The offset cannot be negative.".to_owned())
            } else {
                Ok(ResetMode::Offset(n))
            }
        }),
        "time" => parse_time(value).map(ResetMode::Time).ok_or_else(|| {
            "A time is written like 2026-11-06 00:14, or as milliseconds since the epoch."
                .to_owned()
        }),
        "shift" => number("The shift").map(ResetMode::Shift),
        "current" => Ok(ResetMode::Current),
        _ => Ok(ResetMode::Latest),
    }
}

/// The words of a mode in the call's first line.
fn asked(mode: ResetMode) -> &'static str {
    match mode {
        ResetMode::Earliest => "earliest",
        ResetMode::Latest => "latest",
        ResetMode::Time(_) => "at the time asked",
        ResetMode::Offset(_) | ResetMode::Shift(_) | ResetMode::Current => "earliest and latest",
    }
}

fn clamp(n: i64, start: i64, end: i64) -> i64 {
    n.clamp(start, end.max(start))
}

/// Where each partition of the topic would be after the reset, from OffsetFetch and ListOffsets.
async fn plan(
    b: &Backend,
    topic: &str,
    partitions: usize,
    committed: &BTreeMap<i32, i64>,
    mode: ResetMode,
) -> Vec<ResetRow> {
    let ps: BTreeSet<i32> = (0..i32::try_from(partitions).unwrap_or(i32::MAX)).collect();
    let one: BTreeMap<String, BTreeSet<i32>> = [(topic.to_owned(), ps)].into();
    let ends = read::log_bounds(b, &one, -1).await;
    let starts = read::log_bounds(b, &one, -2).await;
    let at = match mode {
        ResetMode::Time(ms) => read::log_bounds(b, &one, ms).await,
        _ => HashMap::new(),
    };
    plan_rows(
        topic,
        partitions,
        committed,
        mode,
        &Bounds { starts, ends, at },
    )
}

/// What ListOffsets answered for a reset: log starts, log ends and, for a time, the first offset at or after it.
#[derive(Debug, Clone, Default)]
pub struct Bounds {
    pub starts: HashMap<(String, i32), i64>,
    pub ends: HashMap<(String, i32), i64>,
    pub at: HashMap<(String, i32), i64>,
}

/// The preview, from the answers (the pure half; the fixtures run it too).
#[must_use]
pub fn plan_rows(
    topic: &str,
    partitions: usize,
    committed: &BTreeMap<i32, i64>,
    mode: ResetMode,
    bounds: &Bounds,
) -> Vec<ResetRow> {
    (0..i32::try_from(partitions).unwrap_or(i32::MAX))
        .map(|p| {
            let key = (topic.to_owned(), p);
            let (start, end) = (
                bounds.starts.get(&key).copied(),
                bounds.ends.get(&key).copied(),
            );
            let now = committed.get(&p).copied();
            let after = match (mode, start, end) {
                (_, None, _) | (_, _, None) => None,
                (ResetMode::Earliest, Some(s), _) => Some(s),
                (ResetMode::Latest, _, Some(e)) => Some(e),
                (ResetMode::Offset(n), Some(s), Some(e)) => Some(clamp(n, s, e)),
                // A time with no record at or after it answers -1: the log end, as Kafka's own tool does.
                (ResetMode::Time(_), _, Some(e)) => Some(
                    bounds
                        .at
                        .get(&key)
                        .copied()
                        .filter(|o| *o >= 0)
                        .unwrap_or(e),
                ),
                (ResetMode::Shift(n), Some(s), Some(e)) => {
                    now.map(|c| clamp(c.saturating_add(n), s, e))
                }
                (ResetMode::Current, Some(s), Some(e)) => now.map(|c| clamp(c, s, e)),
            };
            ResetRow {
                partition: p,
                committed: now,
                after,
                lag_after: after.map_or(crate::lag::Lag::NoCommit, |a| lag(a, end)),
            }
        })
        .collect()
}

fn topics_with_commits(v: &GroupLagView) -> Vec<(String, Vec<i32>)> {
    v.topics
        .iter()
        .filter_map(|t| {
            let ps: Vec<i32> = t
                .rows
                .iter()
                .filter(|r| r.committed.is_some())
                .map(|r| r.partition)
                .collect();
            (!ps.is_empty()).then(|| (t.name.clone(), ps))
        })
        .collect()
}

/// The reset dialog for `topic` (or the first the group has commits on) and what the radio and value say.
pub async fn reset_form(
    b: &Backend,
    id: &str,
    topic: &str,
    to: &str,
    value: &str,
) -> Option<ResetView> {
    let v = group(b, id, false).await?;
    Some(reset_view(b, v, topic, to, value).await)
}

async fn reset_view(b: &Backend, v: GroupLagView, topic: &str, to: &str, value: &str) -> ResetView {
    let topics: Vec<String> = topics_with_commits(&v)
        .into_iter()
        .map(|(t, _)| t)
        .collect();
    let chosen = if topics.iter().any(|t| t == topic) {
        topic.to_owned()
    } else {
        topics.first().cloned().unwrap_or_default()
    };
    let version = b.version(ApiKey::OffsetCommit);
    let (rows, call, error) = match parse_mode(to, value) {
        Err(e) => (
            Vec::new(),
            calls::reset(version, &v.head.id, &chosen, "", &[]),
            Some(e),
        ),
        Ok(mode) => {
            let block = v.topics.iter().find(|t| t.name == chosen);
            let committed: BTreeMap<i32, i64> = block
                .iter()
                .flat_map(|t| &t.rows)
                .filter_map(|r| r.committed.map(|c| (r.partition, c)))
                .collect();
            let n = block.map_or(0, |t| t.partitions);
            let rows = if chosen.is_empty() {
                Vec::new()
            } else {
                plan(b, &chosen, n, &committed, mode).await
            };
            let moves: Vec<(i32, i64)> = rows
                .iter()
                .filter_map(|r| r.after.map(|a| (r.partition, a)))
                .collect();
            let call = calls::reset(version, &v.head.id, &chosen, asked(mode), &moves);
            (rows, call, None)
        }
    };
    ResetView {
        head: v.head,
        topics,
        topic: chosen,
        to: to.to_owned(),
        value: value.to_owned(),
        rows,
        call,
        error,
    }
}

fn refused(
    head: GroupHead,
    what: &str,
    code: i16,
    call: crate::model::Lines,
    rule_steps: (Vec<crate::model::GroupSpan>, Vec<crate::model::RefusalStep>),
    unchanged: String,
) -> GroupRefused {
    GroupRefused {
        what: what.to_owned(),
        refusal: kafka_refusal(code),
        rule: rule_steps.0,
        steps: rule_steps.1,
        call,
        unchanged,
        head,
    }
}

fn unchanged(h: &GroupHead, v: &GroupLagView) -> String {
    let behind: Vec<String> = v
        .topics
        .iter()
        .filter(|t| t.behind > 0)
        .map(|t| format!("{} behind on {}", crate::format::number(t.behind), t.name))
        .collect();
    if behind.is_empty() {
        format!(
            "{} committed {}",
            h.commits,
            if h.commits == 1 { "offset" } else { "offsets" }
        )
    } else {
        behind.join(", ")
    }
}

/// Applies the reset the dialog describes: the plan is made again here, and what is sent is that plan.
pub async fn reset(b: &Backend, id: &str, topic: &str, to: &str, value: &str) -> ResetResult {
    let Some(v) = group(b, id, false).await else {
        return ResetResult::NoGroup;
    };
    let note = unchanged(&v.head, &v);
    let view = reset_view(b, v, topic, to, value).await;
    let moves: Vec<(i32, i64)> = view
        .rows
        .iter()
        .filter_map(|r| r.after.map(|a| (r.partition, a)))
        .collect();
    if view.error.is_some() || view.topic.is_empty() || moves.is_empty() {
        let mut view = view;
        if view.error.is_none() {
            view.error =
                Some("There is nothing to move: the group has no commit on a topic.".to_owned());
        }
        return ResetResult::Invalid(Box::new(view));
    }
    let Ok(Response::OffsetCommit(r)) = b
        .call(requests::admin_commit(id, &view.topic, &moves))
        .await
    else {
        return ResetResult::NoGroup;
    };
    let code = r
        .topics
        .iter()
        .flat_map(|t| &t.partitions)
        .map(|p| p.error_code)
        .find(|c| *c != 0);
    match code {
        None => ResetResult::Done(moves.len()),
        Some(code) => {
            let version = b.version(ApiKey::OffsetCommit);
            let rule_steps = reset_reading(&view.head, code);
            let refusal = kafka_refusal(code);
            let call = calls::reset_refused(version, id, &view.topic, &moves, &refusal);
            ResetResult::Refused(Box::new(refused(
                view.head,
                "Reset offsets",
                code,
                call,
                rule_steps,
                note,
            )))
        }
    }
}

/// The delete-group dialog.
pub async fn delete_form(b: &Backend, id: &str) -> Option<GroupDeleteView> {
    let v = group(b, id, false).await?;
    let offsets = topics_with_commits(&v);
    Some(GroupDeleteView {
        call: calls::delete_group(b.version(ApiKey::DeleteGroups), id, None),
        offsets,
        head: v.head,
    })
}

/// Deletes the group: Kafka accepts only an empty one.
pub async fn delete(b: &Backend, id: &str) -> DeleteGroupResult {
    let Some(v) = group(b, id, false).await else {
        return DeleteGroupResult::NoGroup;
    };
    let note = unchanged(&v.head, &v);
    let Ok(Response::DeleteGroups(r)) = b.call(requests::delete_group(id)).await else {
        return DeleteGroupResult::NoGroup;
    };
    let code = r.results.first().map_or(0, |x| x.error_code);
    if code == 0 {
        return DeleteGroupResult::Done;
    }
    let refusal = kafka_refusal(code);
    let call = calls::delete_group(b.version(ApiKey::DeleteGroups), id, Some(&refusal));
    let rs = delete_reading(&v.head, code);
    DeleteGroupResult::Refused(Box::new(refused(
        v.head,
        "Delete group",
        code,
        call,
        rs,
        note,
    )))
}

/// The delete-offsets dialog, with `chosen` topics ticked (all, if none is named).
pub async fn offset_delete_form(
    b: &Backend,
    id: &str,
    chosen: &[String],
) -> Option<OffsetDeleteView> {
    let v = group(b, id, false).await?;
    let topics = topics_with_commits(&v);
    let chosen: Vec<String> = if chosen.is_empty() {
        topics.iter().map(|(t, _)| t.clone()).collect()
    } else {
        chosen.to_vec()
    };
    let picked: Vec<(String, Vec<i32>)> = topics
        .iter()
        .filter(|(t, _)| chosen.contains(t))
        .cloned()
        .collect();
    Some(OffsetDeleteView {
        call: calls::offset_delete(id, &picked),
        chosen,
        topics,
        head: v.head,
    })
}

/// Forgets the group's commits on the topics named.
pub async fn offset_delete(b: &Backend, id: &str, chosen: &[String]) -> OffsetDeleteResult {
    let Some(view) = offset_delete_form(b, id, chosen).await else {
        return OffsetDeleteResult::NoGroup;
    };
    let picked: Vec<(String, Vec<i32>)> = view
        .topics
        .iter()
        .filter(|(t, _)| chosen.contains(t))
        .cloned()
        .collect();
    if picked.is_empty() {
        return OffsetDeleteResult::Nothing(Box::new(view));
    }
    let mut done = 0;
    for (topic, partitions) in &picked {
        let Ok(Response::OffsetDelete(r)) =
            b.call(requests::offset_delete(id, topic, partitions)).await
        else {
            return OffsetDeleteResult::NoGroup;
        };
        let code = std::iter::once(r.error_code)
            .chain(
                r.topics
                    .iter()
                    .flat_map(|t| &t.partitions)
                    .map(|p| p.error_code),
            )
            .find(|c| *c != 0);
        if let Some(code) = code {
            return OffsetDeleteResult::Refused(Box::new(view), kafka_refusal(code));
        }
        done += partitions.len();
    }
    OffsetDeleteResult::Done(done)
}
