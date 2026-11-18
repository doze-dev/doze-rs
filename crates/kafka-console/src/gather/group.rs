//! One group, whole: its members (from the protocol and from `ConsoleView::group_detail`), what it committed, where the
//! logs end, and the lag worked out from the last two. Every page of a group is made from this.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use kafka_broker::{GroupDetail, GroupKind};

use super::configs;
use super::group_read::{self as read, Commits, Described};
use crate::backend::Backend;
use crate::lag::{bar_percent, lag};
use crate::model::{
    GroupHead, GroupLagView, GroupMemberRow, GroupPhase, GroupProtocol, GroupRebalance,
    GroupShareView, LagRow, LagState, LagTopic, ShareRow,
};
use crate::source::now_ms;

/// The most chips the switch-group row carries.
const MOST_CHIPS: usize = 8;

/// What a group page reads before it decides what to say.
#[derive(Debug, Clone)]
pub struct Loaded {
    pub described: Option<Described>,
    pub detail: Option<GroupDetail>,
    pub commits: Commits,
    /// What the group has said of itself since the broker started (none when the broker has no such group in memory).
    pub events: Option<kafka_broker::GroupEvents>,
}

/// Asks the broker about one group; `None` if nothing knows it (or the console may not describe it).
pub async fn load(b: &Backend, id: &str) -> Option<Loaded> {
    let ids = [id.to_owned()];
    let described = read::describe(b, &ids).await.remove(id);
    let detail = b.group_detail(id).await;
    let commits = read::commits(b, &ids).await.remove(id);
    let events = b.group_events(id).await;
    if described.is_none() && detail.is_none() && commits.as_ref().is_none_or(Commits::is_empty) {
        return None;
    }
    Some(Loaded {
        described,
        detail,
        commits: commits.unwrap_or_default(),
        events,
    })
}

fn phase_and_protocol(l: &Loaded) -> (GroupPhase, GroupProtocol) {
    let word = l
        .detail
        .as_ref()
        .map(|d| d.state.as_str())
        .or_else(|| l.described.as_ref().map(|d| d.state.as_str()))
        .unwrap_or("Empty");
    let protocol = match (&l.detail, &l.described) {
        (Some(d), _) if d.kind == GroupKind::Consumer => GroupProtocol::Consumer,
        (Some(d), _) if d.kind == GroupKind::Share => GroupProtocol::Share,
        (Some(d), _) if d.kind == GroupKind::Streams => GroupProtocol::Streams,
        (_, Some(d)) => d.protocol,
        _ => GroupProtocol::Classic,
    };
    (GroupPhase::of(word), protocol)
}

/// The members, the protocol's rows with the hidden facts joined by id.
fn members(l: &Loaded, protocol: GroupProtocol, generation: i32) -> Vec<GroupMemberRow> {
    let mut rows = l
        .described
        .as_ref()
        .map(|d| d.members.clone())
        .unwrap_or_default();
    for row in &mut rows {
        let Some(m) = l
            .detail
            .as_ref()
            .and_then(|d| d.members.iter().find(|m| m.id == row.id))
        else {
            continue;
        };
        row.leader = m.leader;
        row.session_ms = m.session_ms;
        row.rebalance_ms = m.rebalance_ms;
        row.heard_ago_ms = Some(m.heard_ago_ms);
        row.rejoined = m.rejoined;
        if protocol == GroupProtocol::Classic {
            row.epoch = generation;
        }
    }
    rows
}

fn rebalance(d: &GroupDetail) -> Option<GroupRebalance> {
    d.rebalance.as_ref().map(|r| GroupRebalance {
        began_ago_ms: r.began_ago_ms,
        remaining_ms: r.remaining_ms,
        timeout_ms: r.timeout_ms,
        waiting_for: r.waiting_for.clone(),
    })
}

/// The chips of the switch-group row: this group and the first others, by id.
fn chips(all: &[read::Listed], this: &str) -> Vec<String> {
    let mut out: Vec<String> = all
        .iter()
        .map(|g| g.id.clone())
        .filter(|g| g != this)
        .take(MOST_CHIPS - 1)
        .collect();
    out.push(this.to_owned());
    out.sort();
    out
}

/// The topics a group is about: committed on, held by a member, or subscribed to.
pub fn topics_of(l: &Loaded) -> BTreeSet<String> {
    let mut out: BTreeSet<String> = l.commits.keys().map(|(t, _)| t.clone()).collect();
    if let Some(d) = &l.described {
        out.extend(d.subscribed.iter().cloned());
        out.extend(
            d.members
                .iter()
                .flat_map(|m| m.holds.iter().chain(&m.target))
                .map(|p| p.topic.clone()),
        );
    }
    out
}

fn holders(l: &Loaded, topic: &str) -> Vec<String> {
    l.described
        .iter()
        .flat_map(|d| &d.members)
        .filter(|m| m.holds.iter().any(|p| p.topic == topic))
        .map(|m| m.id.clone())
        .collect()
}

/// The block of one topic: a row for each of its partitions.
pub fn block(
    name: &str,
    l: &Loaded,
    count: Option<usize>,
    ends: &HashMap<(String, i32), i64>,
    cfg: Option<(bool, Option<i64>)>,
) -> LagTopic {
    let expiry: HashMap<(&str, i32), (i64, Option<i64>)> = l
        .detail
        .iter()
        .flat_map(|d| &d.offsets)
        .map(|o| {
            (
                (o.topic.as_str(), o.partition),
                (o.committed_ms, o.expires_ms),
            )
        })
        .collect();
    let parts: Vec<i32> = match count {
        Some(n) => (0..i32::try_from(n).unwrap_or(i32::MAX)).collect(),
        None => l
            .commits
            .keys()
            .filter(|(t, _)| t == name)
            .map(|(_, p)| *p)
            .collect(),
    };
    let rows: Vec<LagRow> = parts
        .into_iter()
        .map(|p| {
            let committed = l.commits.get(&(name.to_owned(), p)).copied();
            let end = ends.get(&(name.to_owned(), p)).copied();
            let lag = lag(committed.unwrap_or(-1), end);
            let (last, expires) = expiry
                .get(&(name, p))
                .map_or((None, None), |(c, e)| (Some(*c), *e));
            LagRow {
                topic: name.to_owned(),
                partition: p,
                end,
                committed,
                lag,
                bar: bar_percent(lag, end),
                last_commit_ms: last,
                expires_ms: expires,
            }
        })
        .collect();
    LagTopic {
        name: name.to_owned(),
        partitions: count.unwrap_or(rows.len()),
        exists: count.is_some(),
        compacted: cfg.is_some_and(|c| c.0),
        retention_ms: cfg.and_then(|c| c.1),
        holders: holders(l, name),
        behind: rows.iter().map(|r| r.lag.behind()).sum(),
        rows,
    }
}

/// The configs a lag table says something about, by topic: whether it compacts and the retention it sets itself.
pub type Cfgs = HashMap<String, (bool, Option<i64>)>;

/// The blocks of the lag table from what was read (the pure half).
#[must_use]
pub fn topic_blocks(
    l: &Loaded,
    counts: &BTreeMap<String, usize>,
    ends: &HashMap<(String, i32), i64>,
    cfgs: &Cfgs,
) -> Vec<LagTopic> {
    topics_of(l)
        .iter()
        .map(|t| block(t, l, counts.get(t).copied(), ends, cfgs.get(t).copied()))
        .collect()
}

fn head(id: &str, l: &Loaded, chips: Vec<String>, topics: &[LagTopic]) -> GroupHead {
    let (phase, protocol) = phase_and_protocol(l);
    let detail = l.detail.as_ref();
    let generation = detail.map_or(0, |d| d.generation);
    let last_commit_ms = detail.and_then(|d| d.offsets.iter().map(|o| o.committed_ms).max());
    GroupHead {
        id: id.to_owned(),
        protocol,
        phase,
        assignor: detail
            .and_then(|d| d.protocol.clone())
            .or_else(|| l.described.as_ref().and_then(|d| d.assignor.clone())),
        generation,
        target_epoch: detail.and_then(|d| d.assignment_epoch),
        leader: detail.and_then(|d| d.leader.clone()),
        rebalance: detail.and_then(rebalance),
        members: members(l, protocol, generation),
        protocol_type: detail
            .and_then(|d| d.protocol_type.clone())
            .or_else(|| l.described.as_ref().and_then(|d| d.protocol_type.clone())),
        groups: chips,
        lag_total: topics.iter().map(|t| t.behind).sum(),
        commits: l.commits.len(),
        last_commit_ms,
        empty_since_ms: detail.and_then(|d| d.empty_since_ms),
        now_ms: detail.map_or_else(now_ms, |d| d.now_ms),
        session_ms: None,
        story: crate::story::facts_of(l.events.as_ref(), protocol),
    }
}

/// Which state of the Lag page the data calls for; `expiring` is what the address asked for.
#[must_use]
pub fn lag_state(topics: &[LagTopic], expiring: bool) -> LagState {
    use crate::lag::Lag;
    let any = |f: fn(Lag) -> bool| topics.iter().flat_map(|t| &t.rows).any(|r| f(r.lag));
    if expiring {
        LagState::Expiring
    } else if any(|l| matches!(l, Lag::PastEnd(_))) {
        LagState::Ahead
    } else if any(|l| matches!(l, Lag::NoCommit)) {
        LagState::NoCommits
    } else {
        LagState::Normal
    }
}

/// What was read besides the group itself: the groups there are, the partitions of each topic, where logs end, topic configs.
#[derive(Debug, Clone, Copy)]
pub struct Seen<'a> {
    pub all: &'a [read::Listed],
    pub counts: &'a BTreeMap<String, usize>,
    pub ends: &'a HashMap<(String, i32), i64>,
    pub cfgs: &'a Cfgs,
}

/// The whole of one group from what was read: the pure half of [`group`] (the fixtures run it too).
#[must_use]
pub fn assemble(id: &str, l: &Loaded, seen: &Seen<'_>, expiring: bool) -> GroupLagView {
    let topics = topic_blocks(l, seen.counts, seen.ends, seen.cfgs);
    let mut head = head(id, l, chips(seen.all, id), &topics);
    if head.protocol == GroupProtocol::Consumer {
        head.session_ms = head
            .members
            .first()
            .map(|m| m.session_ms)
            .filter(|s| *s > 0);
    }
    GroupLagView {
        state: lag_state(&topics, expiring),
        retention_ms: l.detail.as_ref().map_or(0, |d| d.offsets_retention_ms),
        head,
        topics,
        flash: None,
        share: None,
        streams: None,
    }
}

/// A share group's share-partitions as its page shows them, with the log's end and the lag from the start.
#[must_use]
pub fn share_view(
    v: &kafka_broker::ShareGroupView,
    ends: &HashMap<(String, i32), i64>,
) -> GroupShareView {
    let rows = v
        .partitions
        .iter()
        .map(|p| {
            let log_end = ends.get(&(p.topic.clone(), p.partition)).copied();
            let done = i64::try_from(p.acknowledged + p.archived).unwrap_or(i64::MAX);
            let lag = match (p.start, log_end) {
                (Some(start), Some(end)) => Some((end - start - done).max(0)),
                _ => None,
            };
            ShareRow {
                topic: p.topic.clone(),
                partition: p.partition,
                start: p.start,
                end: p.end,
                log_end,
                lag,
                live: p.live,
                available: p.available,
                acquired: p.acquired,
                acknowledged: p.acknowledged,
                archived: p.archived,
                deliveries: p.deliveries.iter().map(|(c, n)| (*c, *n)).collect(),
                holders: p.holders.iter().map(|(m, n)| (m.clone(), *n)).collect(),
                next_lock_ms: p.next_lock_ms,
            }
        })
        .collect();
    GroupShareView {
        rows,
        delivery_limit: v.delivery_limit,
        lock_ms: v.lock_ms,
        record_locks: v.record_locks,
        reset: v.reset.clone(),
        read_committed: v.read_committed,
    }
}

/// The group `id` with its lag, or `None` if there is no such group (or it may not be described).
pub async fn group(b: &Backend, id: &str, expiring: bool) -> Option<GroupLagView> {
    let l = load(b, id).await?;
    let counts = read::partition_counts(b).await;
    let mut asked: BTreeMap<String, BTreeSet<i32>> = BTreeMap::new();
    for t in topics_of(&l) {
        if let Some(n) = counts.get(&t) {
            asked.insert(t, (0..i32::try_from(*n).unwrap_or(i32::MAX)).collect());
        }
    }
    let ends = read::log_bounds(b, &asked, -1).await;
    let names: Vec<String> = asked.keys().cloned().collect();
    let cfgs: Cfgs = configs::of(b, &names)
        .await
        .into_iter()
        .map(|(k, c)| {
            (
                k,
                (
                    !matches!(c.cleanup, crate::model::Cleanup::Delete),
                    c.retention_set.then_some(c.retention_ms),
                ),
            )
        })
        .collect();
    let all = read::listed(b).await;
    let mut view = assemble(
        id,
        &l,
        &Seen {
            all: &all,
            counts: &counts,
            ends: &ends,
            cfgs: &cfgs,
        },
        expiring,
    );
    // A share group commits nothing: its share-partitions take the place of the lag table.
    if view.head.protocol == GroupProtocol::Share {
        view.share = b.share_group(id).map(|s| share_view(&s, &ends));
        view.head.lag_total = view
            .share
            .as_ref()
            .map_or(0, |s| s.rows.iter().filter_map(|r| r.lag).sum());
        view.topics.clear();
        view.state = LagState::Normal;
    }
    if view.head.protocol == GroupProtocol::Streams {
        view.streams = b
            .streams_group(id)
            .await
            .map(|s| super::group_streams::streams_view(&s, &counts));
    }
    Some(view)
}
