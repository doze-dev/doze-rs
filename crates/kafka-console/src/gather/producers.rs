//! The Producers pages: the producers a partition remembers and the window of batches behind each (the broker's view), the
//! transactions (ListTransactions and DescribeTransactions) and what an open one holds back.

use kafka_broker::{LogFacts, PartitionProducers};
use kafka_wire::api::Response;

use crate::backend::{Backend, requests};
use crate::model::{
    ProdAborted, ProdBatch, ProdCell, ProdRow, ProdTail, ProdTxn, ProducersView, TxnPart, TxnRow,
    TxnView,
};

/// The most transactions the page lists and describes.
pub(super) const MOST_TXNS: usize = 100;
/// The most cells of the tail of a partition.
const MOST_CELLS: i64 = 64;
/// The cells of the tail when nothing is held back.
const USUAL_CELLS: i64 = 19;

fn row_of(t: &kafka_wire::messages::describe_transactions_response::TransactionState) -> TxnRow {
    TxnRow {
        id: t.transactional_id.0.to_string(),
        state: t.transaction_state.to_string(),
        producer_id: t.producer_id.0,
        producer_epoch: t.producer_epoch,
        since_ms: t.transaction_start_time_ms,
        timeout_ms: i64::from(t.transaction_timeout_ms),
        partitions: t
            .topics
            .iter()
            .flat_map(|x| {
                x.partitions
                    .iter()
                    .map(move |p| (x.topic.0.to_string(), *p))
            })
            .collect(),
    }
}

async fn described(b: &Backend, ids: &[String]) -> Vec<TxnRow> {
    let mut out = Vec::new();
    for chunk in ids.chunks(requests::DESCRIBE_CHUNK) {
        if let Ok(Response::DescribeTransactions(d)) =
            b.call(requests::describe_transactions(chunk)).await
        {
            out.extend(
                d.transaction_states
                    .iter()
                    .filter(|t| t.error_code == 0)
                    .map(row_of),
            );
        }
    }
    out
}

/// Every transaction the broker knows (at most [`MOST_TXNS`]), by id, and how many more there are.
pub async fn all_txns(b: &Backend) -> (Vec<TxnRow>, usize) {
    let Ok(Response::ListTransactions(list)) = b.call(requests::list_all_transactions()).await
    else {
        return (Vec::new(), 0);
    };
    let mut ids: Vec<String> = list
        .transaction_states
        .iter()
        .map(|t| t.transactional_id.0.to_string())
        .collect();
    ids.sort();
    let more = ids.len().saturating_sub(MOST_TXNS);
    ids.truncate(MOST_TXNS);
    (described(b, &ids).await, more)
}

/// The cells of the tail of a partition: from where the oldest held-back offset is (a little before it) to the end.
fn tail(f: Option<&LogFacts>, p: (i64, i64, i64)) -> ProdTail {
    let (start, end, stable) = p;
    let from = (end - USUAL_CELLS)
        .min(stable - 6)
        .max(end - MOST_CELLS)
        .max(start);
    let markers: Vec<i64> = f
        .into_iter()
        .flat_map(|f| {
            f.segments
                .iter()
                .flat_map(|s| s.markers.iter().map(|m| m.offset))
        })
        .collect();
    let cells = (from..end)
        .map(|o| {
            if markers.contains(&o) {
                ProdCell::Marker
            } else if o >= stable {
                ProdCell::Held
            } else {
                ProdCell::Committed
            }
        })
        .collect();
    ProdTail { from, cells }
}

fn marker_time(f: Option<&LogFacts>, offset: i64) -> Option<i64> {
    f?.segments
        .iter()
        .flat_map(|s| s.markers.iter())
        .find(|m| m.offset == offset)
        .map(|m| m.at_ms)
}

fn producer_row(
    pp: &PartitionProducers,
    w: &kafka_broker::ProducerWindow,
    open: &[TxnRow],
) -> ProdRow {
    ProdRow {
        id: w.id,
        epoch: w.epoch,
        last_sequence: w.last_sequence,
        last_used_ms: w.last_used_ms,
        txn: open
            .iter()
            .find(|t| t.producer_id == w.id)
            .map(|t| ProdTxn {
                id: t.id.clone(),
                state: t.state.clone(),
                from: w.open_from,
            }),
        batches: w
            .batches
            .iter()
            .map(|x| ProdBatch {
                first_sequence: x.first_sequence,
                last_sequence: x.last_sequence,
                first_offset: x.first_offset,
                last_offset: x.last_offset,
                at_ms: x.at_ms,
                open: w.open_from.is_some_and(|f| x.first_offset >= f) && pp.last_stable < pp.end,
            })
            .collect(),
    }
}

/// The Producers page of `partition` of `topic` (partition 0 if the topic has no such one); `None` if there is no such topic or
/// the console may not read it.
pub async fn producers(
    b: &Backend,
    topic: &str,
    partition: i32,
    opened: Vec<i64>,
) -> Option<ProducersView> {
    let (pp, partition) = match b.partition_producers(topic, partition).await {
        Some(p) => (p, partition),
        None if partition != 0 => (b.partition_producers(topic, 0).await?, 0),
        None => return None,
    };
    let facts = b.log_facts(topic, partition).await;
    let (all, _) = all_txns(b).await;
    let open: Vec<TxnRow> = all
        .into_iter()
        .filter(|t| t.ongoing() && t.partitions.iter().any(|(name, _)| name == topic))
        .collect();
    Some(ProducersView {
        topic: topic.to_owned(),
        partitions: facts.as_ref().map_or(1, |f| f.partitions),
        partition,
        now_ms: pp.now_ms,
        start: pp.start,
        end: pp.end,
        last_stable: pp.last_stable,
        producers: pp
            .producers
            .iter()
            .map(|w| producer_row(&pp, w, &open))
            .collect(),
        aborted: pp
            .aborted
            .iter()
            .map(|a| ProdAborted {
                producer: a.producer,
                partition,
                first: a.first,
                marker: a.marker,
                at_ms: marker_time(facts.as_ref(), a.marker),
            })
            .collect(),
        tail: tail(facts.as_ref(), (pp.start, pp.end, pp.last_stable)),
        open,
        opened,
    })
}

/// The page of the transaction `id` on `topic`; `None` if the topic is not there or the broker knows no such transaction.
pub async fn txn(b: &Backend, topic: &str, id: &str) -> Option<TxnView> {
    let mine = described(b, &[id.to_owned()]).await.into_iter().next()?;
    let (all, more) = all_txns(b).await;
    let mut parts = Vec::new();
    let mut elsewhere = Vec::new();
    for (t, p) in &mine.partitions {
        if t != topic {
            elsewhere.push((t.clone(), *p));
            continue;
        }
        let Some(pp) = b.partition_producers(topic, *p).await else {
            continue;
        };
        parts.push(TxnPart {
            partition: *p,
            first: pp
                .producers
                .iter()
                .find(|w| w.id == mine.producer_id)
                .and_then(|w| w.open_from),
            start: pp.start,
            end: pp.end,
            last_stable: pp.last_stable,
        });
    }
    if parts.is_empty() && elsewhere.is_empty() && mine.partitions.is_empty() {
        // A transaction with no partition yet holds nothing back anywhere; the page still says so.
    }
    let held = parts
        .iter()
        .filter(|p| p.held() > 0)
        .max_by_key(|p| p.held());
    let shown = held.or(parts.first());
    let facts = match shown {
        Some(p) => b.log_facts(topic, p.partition).await,
        None => None,
    };
    let tail = shown.map_or(
        ProdTail {
            from: 0,
            cells: Vec::new(),
        },
        |p| tail(facts.as_ref(), (p.start, p.end, p.last_stable)),
    );
    Some(TxnView {
        topic: topic.to_owned(),
        partitions: facts.as_ref().map_or(1, |f| f.partitions),
        now_ms: crate::source::now_ms(),
        tail_partition: shown.map_or(0, |p| p.partition),
        txn: mine,
        parts,
        elsewhere,
        tail,
        all,
        more,
    })
}
