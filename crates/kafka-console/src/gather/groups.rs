//! The Groups list: every group with its state, members, what it reads and how far behind it is.
//!
//! [`view`] is the pure half (what the fixtures run too); [`groups`] asks the broker and hands it the answers.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use super::group::{Loaded, block, topics_of};
use super::group_read as read;
use crate::backend::Backend;
use crate::lag::Lag;
use crate::model::{
    GroupAttention, GroupFilter, GroupPhase, GroupProtocol, GroupRow, GroupsView, LagTopic, RowLag,
};
use crate::source::now_ms;

/// The most rows a page shows (and so the most groups looked into for their lag).
pub const MOST_ROWS: usize = 100;

/// A group as the list reads it.
#[derive(Debug, Clone)]
pub struct Entry {
    pub listed: read::Listed,
    pub loaded: Loaded,
}

fn row_lag(topics: &[LagTopic], any_commit: bool) -> RowLag {
    let behind: Vec<i64> = topics
        .iter()
        .flat_map(|t| &t.rows)
        .filter(|r| matches!(r.lag, Lag::Behind(_)))
        .map(|r| r.lag.behind())
        .collect();
    let each = behind
        .first()
        .copied()
        .filter(|first| behind.len() > 1 && behind.iter().all(|n| n == first));
    let ahead = topics
        .iter()
        .flat_map(|t| &t.rows)
        .find_map(|r| match r.lag {
            Lag::PastEnd(by) => Some((format!("{}-{}", r.topic, r.partition), by)),
            _ => None,
        });
    let uncommitted = topics
        .iter()
        .filter(|t| t.rows.iter().any(|r| r.lag == Lag::NoCommit))
        .map(|t| t.name.clone())
        .collect();
    RowLag {
        total: behind.iter().sum(),
        each,
        ahead,
        uncommitted,
        any_commit,
    }
}

/// How many groups are in each state: all, stable, rebalancing, empty.
#[must_use]
pub fn counts(phases: &[GroupPhase]) -> [usize; 4] {
    let n = |f: fn(GroupPhase) -> bool| phases.iter().filter(|p| f(**p)).count();
    [
        phases.len(),
        n(|p| p == GroupPhase::Stable),
        n(|p| !matches!(p, GroupPhase::Stable | GroupPhase::Empty)),
        n(|p| p == GroupPhase::Empty),
    ]
}

/// The phase and protocol a group is in, from what the protocol said of it.
#[must_use]
pub fn phase_of(listed: &read::Listed, loaded: &Loaded) -> (GroupPhase, GroupProtocol) {
    let d = loaded.described.as_ref();
    let word = loaded
        .detail
        .as_ref()
        .map(|x| x.state.as_str())
        .or_else(|| d.map(|x| x.state.as_str()));
    (
        word.map_or(GroupPhase::Empty, GroupPhase::of),
        d.map_or(listed.protocol, |x| x.protocol),
    )
}

fn row(e: &Entry, topics: &[LagTopic]) -> GroupRow {
    let (phase, protocol) = phase_of(&e.listed, &e.loaded);
    let d = e.loaded.described.as_ref();
    let detail = e.loaded.detail.as_ref();
    GroupRow {
        id: e.listed.id.clone(),
        protocol,
        assignor: detail
            .and_then(|x| x.protocol.clone())
            .or_else(|| d.and_then(|x| x.assignor.clone())),
        phase,
        generation: detail.map_or(0, |x| x.generation),
        members: d.map_or(0, |x| x.members.len()),
        reads: topics
            .iter()
            .filter(|t| t.rows.iter().any(|r| r.committed.is_some()) || !t.holders.is_empty())
            .map(|t| t.name.clone())
            .collect(),
        lag: row_lag(topics, !e.loaded.commits.is_empty()),
        last_commit_ms: detail.and_then(|x| x.offsets.iter().map(|o| o.committed_ms).max()),
        expires_ms: detail.and_then(|x| x.offsets.iter().filter_map(|o| o.expires_ms).min()),
    }
}

fn attention(entries: &[Entry]) -> Option<GroupAttention> {
    entries.iter().find_map(|e| {
        let (phase, protocol) = phase_of(&e.listed, &e.loaded);
        if protocol != GroupProtocol::Classic || phase != GroupPhase::Rebalancing {
            return None;
        }
        let r = e.loaded.detail.as_ref()?.rebalance.as_ref()?;
        Some(GroupAttention {
            id: e.listed.id.clone(),
            waiting_for: r.waiting_for.clone(),
            remaining_ms: r.remaining_ms,
            timeout_ms: r.timeout_ms,
        })
    })
}

/// What the list says of a set of groups: `entries` are the ones shown (already filtered, by id), `phases` the state of every
/// group the name filter matched (for the chips), `total` how many groups there are.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn view(
    filter: &str,
    state: GroupFilter,
    total: usize,
    phases: &[GroupPhase],
    entries: &[Entry],
    truncated: bool,
    partitions: &BTreeMap<String, usize>,
    ends: &HashMap<(String, i32), i64>,
    now_ms: i64,
) -> GroupsView {
    let rows = entries
        .iter()
        .map(|e| {
            let topics: Vec<LagTopic> = topics_of(&e.loaded)
                .iter()
                .map(|t| block(t, &e.loaded, partitions.get(t).copied(), ends, None))
                .collect();
            row(e, &topics)
        })
        .collect();
    GroupsView {
        rows,
        total,
        counts: counts(phases),
        filter: filter.to_owned(),
        state,
        topics: partitions.len(),
        attention: attention(entries),
        now_ms,
        truncated,
    }
}

/// The list, for a name filter and a state chip.
pub async fn groups(b: &Backend, filter: &str, state: GroupFilter) -> GroupsView {
    let filter: String = filter.chars().take(255).collect();
    let all = read::listed(b).await;
    let named: Vec<&read::Listed> = all
        .iter()
        .filter(|g| filter.is_empty() || g.id.contains(filter.as_str()))
        .collect();
    let ids: Vec<String> = named.iter().map(|g| g.id.clone()).collect();
    let mut described = HashMap::new();
    for chunk in ids.chunks(crate::backend::requests::MOST_DESCRIBED).take(2) {
        described.extend(read::describe(b, chunk).await);
    }
    // A group that is listed but not described (the protocol has no word for it yet) is Empty.
    let phase = |g: &read::Listed| {
        described
            .get(&g.id)
            .map_or(GroupPhase::Empty, |d| GroupPhase::of(&d.state))
    };
    let phases: Vec<GroupPhase> = named.iter().map(|g| phase(g)).collect();
    let chosen: Vec<&read::Listed> = named
        .iter()
        .zip(&phases)
        .filter(|(_, p)| state.takes(**p))
        .map(|(g, _)| *g)
        .collect();
    let truncated = chosen.len() > MOST_ROWS;
    let shown: Vec<&read::Listed> = chosen.into_iter().take(MOST_ROWS).collect();
    let shown_ids: Vec<String> = shown.iter().map(|g| g.id.clone()).collect();
    let mut commits = read::commits(b, &shown_ids).await;
    let partitions = read::partition_counts(b).await;
    let mut entries = Vec::new();
    for g in shown {
        entries.push(Entry {
            listed: g.clone(),
            loaded: Loaded {
                described: described.get(&g.id).cloned(),
                detail: b.group_detail(&g.id).await,
                commits: commits.remove(&g.id).unwrap_or_default(),
            },
        });
    }
    let mut asked: BTreeMap<String, BTreeSet<i32>> = BTreeMap::new();
    for e in &entries {
        for t in topics_of(&e.loaded) {
            if let Some(n) = partitions.get(&t) {
                asked.insert(t, (0..i32::try_from(*n).unwrap_or(i32::MAX)).collect());
            }
        }
    }
    let ends = read::log_bounds(b, &asked, -1).await;
    view(
        &filter,
        state,
        all.len(),
        &phases,
        &entries,
        truncated,
        &partitions,
        &ends,
        now_ms(),
    )
}
