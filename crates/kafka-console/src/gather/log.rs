//! The Log page: the broker's facts about one partition's log as the page reads them, and a line for each of a few other topics.

use kafka_broker::{CleanerState, LogFacts};
use kafka_storage::SegmentWalk;

use super::meta;
use crate::backend::Backend;
use crate::format::{bytes, count, number, span_ms};
use crate::log_words::{expiry, pct, ratio_text};
use crate::model::{
    Cleanup, LogCleaner, LogCompaction, LogDurability, LogLimit, LogMoved, LogOther, LogOtherLink,
    LogPass, LogPlanner, LogRetentionView, LogRound, LogRules, LogSegment, LogState, LogStop,
    LogTombstone, LogView,
};

/// How many other topics the page lists.
const MOST_OTHERS: usize = 6;

fn segment(s: &SegmentWalk, active: bool) -> LogSegment {
    LogSegment {
        base: s.base,
        next: s.next,
        bytes: s.bytes,
        newest_ms: s.newest_ms.or(s.modified_ms),
        records: s.records,
        gaps: s.gaps.clone(),
        unplaced: s.unplaced,
        exact: s.walked,
        active,
    }
}

fn compaction(c: kafka_broker::Compaction) -> LogCompaction {
    let r = c.reading;
    LogCompaction {
        rules: LogRules {
            min_dirty_ratio: c.min_dirty_ratio,
            min_lag_ms: c.min_lag_ms,
            max_lag_ms: c.max_lag_ms,
            delete_retention_ms: c.delete_retention_ms,
        },
        every_ms: c.every_ms,
        looked_at_ms: c.looked_at_ms,
        planner: LogPlanner {
            upto: r.upto,
            limit: match r.limit {
                kafka_storage::compact::Limit::Active => LogLimit::Active,
                kafka_storage::compact::Limit::Transaction => LogLimit::Transaction,
                kafka_storage::compact::Limit::TooNew => LogLimit::TooNew,
            },
            first_dirty: r.first_dirty,
            dirty_start: r.dirty_start,
            dirty_bytes: r.dirty_bytes,
            clean_bytes: r.clean_bytes,
            waited_too_long: r.waited_too_long,
            due: c.verdict.is_some(),
        },
        cleaner: match c.state {
            CleanerState::Disabled => LogCleaner::Disabled,
            CleanerState::Running { since_ms } => LogCleaner::Running { since_ms },
            CleanerState::Failed { at_ms, message } => LogCleaner::Failed { at_ms, message },
            CleanerState::Watching => LogCleaner::Watching,
        },
        last: c.last.map(|n| LogRound {
            at_ms: n.at_ms,
            took_ms: n.took_ms,
            read: n.read,
            kept: n.kept,
            replaced: n.replaced,
            clean_to: n.clean_to,
        }),
    }
}

fn retention(r: kafka_broker::LogRetention) -> LogRetentionView {
    LogRetentionView {
        max_age_ms: r.max_age_ms,
        max_bytes: r.max_bytes,
        every_ms: r.every_ms,
        would_delete: r.would_delete,
        last_pass: r.last_pass.map(|p| LogPass {
            at_ms: p.at_ms,
            looked: p.looked,
            moved: p
                .deleted
                .into_iter()
                .map(|d| LogMoved {
                    topic: d.topic,
                    partition: d.partition,
                    from: d.from,
                    to: d.to,
                })
                .collect(),
            more: p.more,
        }),
        moved_total: r.moved_total,
        moved_at_ms: r.moved_at_ms,
    }
}

fn tombstone(t: &kafka_storage::Tombstone) -> LogTombstone {
    let text = String::from_utf8(t.key.clone());
    LogTombstone {
        binary: text.is_err(),
        key: text.unwrap_or_else(|_| String::from_utf8_lossy(&t.key).into_owned()),
        offset: t.offset,
        written_ms: t.written_ms,
        goes_after_ms: t.goes_after_ms,
    }
}

/// The page's model from the broker's facts.
#[must_use]
pub fn view_of(f: LogFacts) -> LogView {
    let last = f.segments.len().saturating_sub(1);
    let cleanup = match (f.policy.compact, f.policy.delete) {
        (true, true) => Cleanup::Both,
        (true, false) => Cleanup::Compact,
        _ => Cleanup::Delete,
    };
    LogView {
        topic: f.topic,
        partition: f.partition,
        partitions: f.partitions,
        now_ms: f.now_ms,
        cleanup,
        start: f.start,
        end: f.end,
        last_stable: f.last_stable,
        never_written: f.never_written,
        segments: f
            .segments
            .iter()
            .enumerate()
            .map(|(i, s)| segment(s, i == last))
            .collect(),
        partial: f.partial,
        segment_bytes: f.segment_bytes,
        index_interval_bytes: f.index_interval_bytes,
        segment_ms: f.segment_ms,
        checkpoint: f.checkpoint,
        tombstones: f.tombstones.iter().map(tombstone).collect(),
        compaction: f.compaction.map(compaction),
        retention: f.retention.map(retention),
        durability: LogDurability {
            offline: f.durability.offline,
            stop: match f.durability.last_stop {
                kafka_broker::LastStop::None => LogStop::None,
                kafka_broker::LastStop::Clean => LogStop::Clean,
                kafka_broker::LastStop::Unclean => LogStop::Unclean,
            },
            fsync_ms: f.durability.fsync_ms,
        },
        others: Vec::new(),
    }
}

/// What a line of "Other logs" says of a topic, and the button beside it.
#[must_use]
pub fn other_of(v: &LogView) -> LogOther {
    let n = v.segments.len();
    let segs = count(n.max(1), "segment", "segments");
    let (line, sub, link) = match (v.state(), &v.compaction, &v.retention) {
        (LogState::Empty, _, _) => (
            "empty: no records yet".to_owned(),
            Some("one active segment, nothing else".to_owned()),
            LogOtherLink::Empty,
        ),
        (LogState::NotDue, Some(c), _) => (
            match c.planner.ratio() {
                Some(r) => format!("{segs}, {} dirty: not due", pct(r)),
                None => format!("{segs}, nothing dirty: not due"),
            },
            Some(format!("needs more than {}", pct(c.rules.min_dirty_ratio))),
            LogOtherLink::NotDue,
        ),
        (LogState::Due, Some(c), _) => (
            format!(
                "{segs}, {} dirty: due",
                c.planner.ratio().map_or_else(String::new, pct)
            ),
            Some(format!("ratio {}", ratio_text(c.rules.min_dirty_ratio))),
            LogOtherLink::Plain,
        ),
        (_, _, Some(r)) => {
            let line = match (r.max_age_ms, expiry(v)) {
                (Some(age), Some(e)) if e.going_now > 0 => {
                    format!("retention {}: segment 1 is past it", span_ms(age))
                }
                (Some(age), _) => {
                    format!("retention {}: {segs}, {}", span_ms(age), bytes(v.bytes()))
                }
                (None, _) => format!("kept for ever: {segs}, {}", bytes(v.bytes())),
            };
            (
                line,
                Some(format!("checked every {}", span_ms(r.every_ms))),
                LogOtherLink::Retention,
            )
        }
        _ => (
            format!("{segs}, {}", number(v.end - v.start)),
            None,
            LogOtherLink::Plain,
        ),
    };
    LogOther {
        topic: v.topic.clone(),
        cleanup: v.cleanup,
        line,
        sub,
        link,
    }
}

/// The log of `partition` of `topic` (partition 0 if the topic has no such one), with a line for a few other topics; `None` if
/// there is no such topic or the console may not describe it.
pub async fn view(b: &Backend, topic: &str, partition: i32) -> Option<LogView> {
    let facts = match b.log_facts(topic, partition).await {
        Some(f) => f,
        None if partition != 0 => b.log_facts(topic, 0).await?,
        None => return None,
    };
    let mut v = view_of(facts);
    v.others = others(b, topic).await;
    Some(v)
}

async fn others(b: &Backend, topic: &str) -> Vec<LogOther> {
    let all = meta::all(b).await.unwrap_or_default();
    let mut out = Vec::new();
    for t in all
        .iter()
        .filter(|t| !t.internal && t.name != topic)
        .take(MOST_OTHERS)
    {
        if let Some(f) = b.log_facts(&t.name, 0).await {
            out.push(other_of(&view_of(f)));
        }
    }
    out
}
