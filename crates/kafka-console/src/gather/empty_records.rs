//! The topic page's Empty (CON-1590): what it removes per partition, who falls below the new start, then one DeleteRecords
//! call of every partition to its high watermark, and (when asked) each group moved to the new start.

use std::collections::BTreeMap;

use kafka_wire::api::Response;
use kafka_wire::messages::ApiKey;

use super::{forms, group_write};
use crate::backend::{Backend, requests::topics_write};
use crate::empty_records::{
    EmptyGroup, EmptyPart, EmptyRecordsView, EmptyResult, GroupMoved, RecordsEmptied,
};
use crate::{calls, refusal};

/// cleanup.policy names `compact` and not `delete`.
fn compacts_only(policy: &str) -> bool {
    let words: Vec<&str> = policy.split(',').map(str::trim).collect();
    words.contains(&"compact") && !words.contains(&"delete")
}

/// The groups with a commit below a partition's high watermark, and how much each had not read.
async fn behind(b: &Backend, topic: &str, ends: &BTreeMap<i32, i64>) -> Vec<EmptyGroup> {
    let parts: Vec<i32> = ends.keys().copied().collect();
    let mut by: BTreeMap<String, Vec<(i32, i64)>> = BTreeMap::new();
    for (g, p, o) in forms::committed_on(b, topic, &parts).await {
        if ends.get(&p).is_some_and(|end| o < *end) {
            by.entry(g).or_default().push((p, o));
        }
    }
    let mut out = Vec::with_capacity(by.len());
    for (group, mut below) in by {
        below.sort_unstable();
        let unread = below
            .iter()
            .map(|(p, o)| ends.get(p).map_or(0, |e| e - o))
            .sum();
        let members = super::group::group(b, &group, false)
            .await
            .map_or(0, |v| v.head.members.len());
        out.push(EmptyGroup {
            group,
            below,
            unread,
            members,
        });
    }
    out
}

/// The dialog for `topic`; `None` if there is no such topic.
pub async fn form(b: &Backend, topic: &str) -> Option<EmptyRecordsView> {
    let facts = forms::topic(b, topic).await?;
    let parts = facts.partitions.clone();
    let starts = forms::offsets(b, topic, &parts, -2, 0).await;
    let ends = forms::offsets(b, topic, &parts, -1, 0).await;
    let files: BTreeMap<i32, u64> = b
        .partition_files(topic)
        .unwrap_or_default()
        .into_iter()
        .map(|f| (f.partition, f.bytes))
        .collect();
    let rows: Vec<EmptyPart> = parts
        .iter()
        .map(|&p| {
            let end = ends.get(&p).copied().unwrap_or(0);
            let start = starts.get(&p).copied().unwrap_or(end).min(end);
            EmptyPart {
                partition: p,
                log_start: start,
                high_watermark: end,
                records: end - start,
                bytes: files.get(&p).copied(),
            }
        })
        .collect();
    let ends: BTreeMap<i32, i64> = rows
        .iter()
        .map(|r| (r.partition, r.high_watermark))
        .collect();
    let policy = forms::configs(b, topic, &["cleanup.policy"]).await;
    let bytes = (!files.is_empty()).then(|| {
        rows.iter()
            .filter(|r| r.records > 0)
            .filter_map(|r| r.bytes)
            .sum()
    });
    Some(EmptyRecordsView {
        topic: topic.to_owned(),
        records: rows.iter().map(|r| r.records).sum(),
        bytes,
        groups: behind(b, topic, &ends).await,
        compact_only: policy
            .get("cleanup.policy")
            .is_some_and(|c| compacts_only(c)),
        call: calls::delete_all_records(b.version(ApiKey::DeleteRecords), topic, &parts),
        parts: rows,
        error: None,
    })
}

/// Moves each of `groups` (those of the dialog only) to the new start of `topic`.
async fn move_groups(
    b: &Backend,
    topic: &str,
    groups: &[String],
    view: &EmptyRecordsView,
) -> Vec<GroupMoved> {
    let mut out = Vec::new();
    for g in groups {
        if !view.groups.iter().any(|x| &x.group == g) {
            continue;
        }
        let (moved, refused) = match group_write::reset(b, g, topic, "earliest", "").await {
            group_write::ResetResult::Done(n) => (n, None),
            group_write::ResetResult::Refused(r) => (
                0,
                Some(format!("{}: {}", r.refusal.name, r.refusal.message)),
            ),
            group_write::ResetResult::Invalid(v) => (0, v.error.clone()),
            group_write::ResetResult::NoGroup => (0, Some("The group is gone.".to_owned())),
        };
        out.push(GroupMoved {
            group: g.clone(),
            moved,
            refused,
        });
    }
    out
}

/// Empties `topic` when `confirm` is its name, then moves `groups` to the new start.
pub async fn empty(b: &Backend, topic: &str, confirm: &str, groups: &[String]) -> EmptyResult {
    let Some(mut view) = form(b, topic).await else {
        return EmptyResult::NoTopic;
    };
    if confirm.trim() != topic {
        view.error = Some(format!("Type {topic} to confirm."));
        return EmptyResult::Invalid(Box::new(view));
    }
    let parts: Vec<i32> = view.parts.iter().map(|p| p.partition).collect();
    let Ok(Response::DeleteRecords(r)) = b
        .call(topics_write::delete_all_records(topic, &parts))
        .await
    else {
        return EmptyResult::NoTopic;
    };
    let answers: Vec<(i32, i64, i16)> = r
        .topics
        .into_iter()
        .flat_map(|t| t.partitions)
        .map(|p| (p.partition_index, p.low_watermark, p.error_code))
        .collect();
    if let Some((_, _, code)) = answers.iter().find(|(_, _, c)| *c != 0) {
        let r = refusal::delete_records(*code, None);
        return EmptyResult::Refused(Box::new(view), r);
    }
    let moved = move_groups(b, topic, groups, &view).await;
    let mut starts: Vec<(i32, i64)> = answers.iter().map(|(p, l, _)| (*p, *l)).collect();
    starts.sort_unstable();
    EmptyResult::Done(RecordsEmptied {
        topic: topic.to_owned(),
        starts,
        records: view.records,
        moved,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_compact_without_delete_is_compact_only() {
        for (p, want) in [
            ("compact", true),
            ("compact, delete", false),
            ("delete,compact", false),
            ("delete", false),
            ("", false),
        ] {
            assert_eq!(super::compacts_only(p), want, "{p}");
        }
    }
}
