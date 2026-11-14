//! The dialogs and refusals of the larder's groups, made from the same assembly as the pages.

use std::collections::BTreeMap;

use super::{ends, group};
use crate::gather::{Bounds, plan_rows};
use crate::groups_words::{delete_reading, kafka_refusal, reset_reading};
use crate::model::{
    GroupDeleteView, GroupLagView, GroupRefused, OffsetDeleteView, ResetMode, ResetView,
};
use crate::{calls_groups as calls, gather};

fn bounds_for(topic: &str, n: usize, at: Option<i64>) -> Bounds {
    let ends = ends();
    let mut b = Bounds::default();
    for p in 0..i32::try_from(n).unwrap_or(0) {
        let key = (topic.to_owned(), p);
        if let Some(e) = ends.get(&key) {
            b.ends.insert(key.clone(), *e);
            // The log starts at 0 in the world, except that clickstream keeps an hour.
            b.starts.insert(key.clone(), 0);
            if let Some(at) = at {
                b.at.insert(key, at);
            }
        }
    }
    b
}

/// The reset dialog of a group, as the page would draw it for `topic` and the radio chosen.
#[must_use]
pub fn reset(id: &str, topic: &str, to: &str, value: &str) -> Option<ResetView> {
    let g = group(id, false)?;
    let topics: Vec<String> = g
        .topics
        .iter()
        .filter(|t| t.rows.iter().any(|r| r.committed.is_some()))
        .map(|t| t.name.clone())
        .collect();
    let chosen = if topics.iter().any(|t| t == topic) {
        topic.to_owned()
    } else {
        topics.first().cloned().unwrap_or_default()
    };
    let mode = gather::parse_mode(to, value);
    let block = g.topics.iter().find(|t| t.name == chosen);
    let committed: BTreeMap<i32, i64> = block
        .iter()
        .flat_map(|t| &t.rows)
        .filter_map(|r| r.committed.map(|c| (r.partition, c)))
        .collect();
    let n = block.map_or(0, |t| t.partitions);
    let (rows, error, call) = match mode {
        Err(e) => (Vec::new(), Some(e), calls::reset(9, id, &chosen, "", &[])),
        Ok(mode) => {
            let at = match mode {
                ResetMode::Time(ms) => Some(ms / 1000),
                _ => None,
            };
            let rows = plan_rows(&chosen, n, &committed, mode, &bounds_for(&chosen, n, at));
            let moves: Vec<(i32, i64)> = rows
                .iter()
                .filter_map(|r| r.after.map(|a| (r.partition, a)))
                .collect();
            let asked = match mode {
                ResetMode::Earliest => "earliest",
                ResetMode::Latest => "latest",
                ResetMode::Time(_) => "at the time asked",
                _ => "earliest and latest",
            };
            (rows, None, calls::reset(9, id, &chosen, asked, &moves))
        }
    };
    Some(ResetView {
        head: g.head,
        topics,
        topic: chosen,
        to: to.to_owned(),
        value: value.to_owned(),
        rows,
        call,
        error,
    })
}

fn unchanged(g: &GroupLagView) -> String {
    let behind: Vec<String> = g
        .topics
        .iter()
        .filter(|t| t.behind > 0)
        .map(|t| format!("{} behind on {}", crate::format::number(t.behind), t.name))
        .collect();
    behind.join(", ")
}

/// The page after Kafka refused a reset (the group has a member).
#[must_use]
pub fn reset_refused(id: &str) -> Option<GroupRefused> {
    let g = group(id, false)?;
    let refusal = kafka_refusal(25);
    let moves: Vec<(i32, i64)> = g
        .topics
        .iter()
        .find(|t| t.rows.iter().any(|r| r.committed.is_some()))
        .map(|t| {
            t.rows
                .iter()
                .filter_map(|r| r.end.map(|e| (r.partition, e)))
                .collect()
        })
        .unwrap_or_default();
    let first = g.topics.first().map_or("", |t| t.name.as_str());
    let (rule, steps) = reset_reading(&g.head, 25);
    Some(GroupRefused {
        what: "Reset offsets".to_owned(),
        call: calls::reset_refused(9, id, first, &moves, &refusal),
        refusal,
        rule,
        steps,
        unchanged: unchanged(&g),
        head: g.head,
    })
}

/// The delete-group dialog.
#[must_use]
pub fn delete_form(id: &str) -> Option<GroupDeleteView> {
    let g = group(id, false)?;
    let offsets = g
        .topics
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
        .collect();
    Some(GroupDeleteView {
        call: calls::delete_group(2, id, None),
        offsets,
        head: g.head,
    })
}

/// The page after Kafka refused a delete (the group has a member).
#[must_use]
pub fn delete_refused(id: &str) -> Option<GroupRefused> {
    let g = group(id, false)?;
    let refusal = kafka_refusal(68);
    let (rule, steps) = delete_reading(&g.head, 68);
    Some(GroupRefused {
        what: "Delete group".to_owned(),
        call: calls::delete_group(2, id, Some(&refusal)),
        refusal,
        rule,
        steps,
        unchanged: unchanged(&g),
        head: g.head,
    })
}

/// The delete-offsets dialog.
#[must_use]
pub fn offset_delete_form(id: &str, chosen: &[String]) -> Option<OffsetDeleteView> {
    let v = delete_form(id)?;
    let chosen: Vec<String> = if chosen.is_empty() {
        v.offsets.iter().map(|(t, _)| t.clone()).collect()
    } else {
        chosen.to_vec()
    };
    let picked: Vec<(String, Vec<i32>)> = v
        .offsets
        .iter()
        .filter(|(t, _)| chosen.contains(t))
        .cloned()
        .collect();
    Some(OffsetDeleteView {
        call: calls::offset_delete(id, &picked),
        topics: v.offsets,
        chosen,
        head: v.head,
    })
}
