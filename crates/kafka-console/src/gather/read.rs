//! Reading a partition through Fetch: one request, or a run of them, bounded in requests and in bytes.

use kafka_wire::Uuid;
use kafka_wire::api::Response;

use super::walk::{Keep, Raw, walk};
use crate::backend::{Backend, requests};
use crate::model::{Reading, Standing};
use requests::{Isolation, TABLE_BYTES};

/// The most Fetch requests one read makes.
const MOST_FETCHES: usize = 16;
/// The most record bytes one read takes in all.
pub const MOST_BYTES: usize = 4 << 20;

/// What one Fetch gave.
#[derive(Debug, Default)]
pub struct Chunk {
    pub rows: Vec<Raw>,
    /// Where the next Fetch begins; `None` if nothing came.
    pub next: Option<i64>,
    pub start: i64,
    pub hw: i64,
    pub lso: i64,
    /// The partition's error code, if the broker refused the read.
    pub error: Option<i16>,
    /// The bytes of records that came.
    pub bytes: usize,
}

fn isolation(r: Reading) -> Isolation {
    match r {
        Reading::Uncommitted => Isolation::ReadUncommitted,
        Reading::Committed => Isolation::ReadCommitted,
    }
}

/// The partition's answer to a Fetch: its data, or why not.
struct Answer {
    records: bytes::Bytes,
    start: i64,
    hw: i64,
    lso: i64,
    aborted: Vec<(i64, i64)>,
    error: Option<i16>,
}

async fn ask(b: &Backend, id: Uuid, partition: i32, from: i64, max: i32, iso: Isolation) -> Answer {
    let empty = Answer {
        records: bytes::Bytes::new(),
        start: 0,
        hw: 0,
        lso: 0,
        aborted: Vec::new(),
        error: Some(-1),
    };
    let Ok(Response::Fetch(r)) = b
        .call(requests::fetch_partition(id, partition, from, max, iso))
        .await
    else {
        return empty;
    };
    let Some(p) = r.responses.first().and_then(|t| t.partitions.first()) else {
        return Answer { error: (r.error_code != 0).then_some(r.error_code).or(empty.error), ..empty };
    };
    Answer {
        records: p.records.clone().unwrap_or_default(),
        start: p.log_start_offset,
        hw: p.high_watermark,
        lso: p.last_stable_offset,
        aborted: p
            .aborted_transactions
            .iter()
            .flatten()
            .map(|a| (a.producer_id.0, a.first_offset))
            .collect(),
        error: (p.error_code != 0).then_some(p.error_code),
    }
}

/// One Fetch of `partition` from `from`, walked: at most `limit` records. In `read_committed` the records of aborted
/// transactions are dropped, as a consumer drops them.
pub async fn once(
    b: &Backend,
    id: Uuid,
    (partition, from): (i32, i64),
    reading: Reading,
    (keep, limit): (Keep, usize),
) -> Chunk {
    let first = ask(b, id, partition, from, TABLE_BYTES, isolation(reading)).await;
    if first.error.is_some() {
        return Chunk { error: first.error, start: first.start, hw: first.hw, lso: first.lso, ..Chunk::default() };
    }
    let mut walked = walk(&first.records, partition, from, first.lso, &first.aborted, (keep, limit));
    let mut aborted = first.aborted;
    if reading == Reading::Uncommitted && walked.saw_txn {
        // A read_uncommitted answer does not list the aborted transactions; ask for the list a consumer of the other
        // level would be given (one batch at most comes with it) and walk again.
        aborted = ask(b, id, partition, from, 1, Isolation::ReadCommitted).await.aborted;
        walked = walk(&first.records, partition, from, first.lso, &aborted, (keep, limit));
    }
    let mut rows = walked.rows;
    if reading == Reading::Committed {
        rows.retain(|r| r.standing == Standing::Committed);
    }
    Chunk {
        rows,
        next: walked.next,
        start: first.start,
        hw: first.hw,
        lso: first.lso,
        error: None,
        bytes: first.records.len(),
    }
}

/// What a run of reads found.
#[derive(Debug, Default)]
pub struct Window {
    pub rows: Vec<Raw>,
    /// The cursor got to `until`.
    pub reached: bool,
    pub bytes: usize,
    pub error: Option<i16>,
}

/// Reads `partition` from `from` forward until the cursor reaches `until`, `want` records are in hand (when given), or a bound
/// is met: 16 requests, or 4 MiB of records.
pub async fn forward(
    b: &Backend,
    id: Uuid,
    (partition, from, until): (i32, i64, i64),
    reading: Reading,
    (keep, want): (Keep, Option<usize>),
) -> Window {
    let mut w = Window::default();
    let mut cursor = from;
    for _ in 0..MOST_FETCHES {
        if cursor >= until || w.bytes >= MOST_BYTES || want.is_some_and(|n| w.rows.len() >= n) {
            break;
        }
        let c = once(b, id, (partition, cursor), reading, (keep, usize::MAX)).await;
        w.error = c.error;
        w.bytes += c.bytes;
        let progressed = c.next.is_some_and(|n| n > cursor);
        w.rows.extend(c.rows.into_iter().filter(|r| r.offset < until));
        match c.next {
            Some(n) if progressed => cursor = n,
            _ => break,
        }
    }
    w.reached = cursor >= until;
    w
}
