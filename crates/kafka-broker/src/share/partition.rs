//! One share-partition: a share group's view of one topic partition (Kafka's `SharePartition`).
//!
//! Records from the share-partition start offset (SPSO, `start`) to the end offset (SPEO, `end`) are *in flight*: each is
//! available, acquired by one member under a lock that runs out, acknowledged, or archived. They are kept as Kafka keeps them,
//! by the batch they were acquired in (`cached`, keyed by first offset), with a state for the whole batch until a request
//! needs one per offset; the shapes of what is acquired and what may be acknowledged follow from that, so they are Kafka's.
//!
//! Pure: no clock (the time is handed in), no store (a [`Snapshot`] is what is written), no log (the batches read are
//! handed in as [`Span`]s). The caller holds it under a lock, writes the snapshot after a change that Kafka persists, and
//! puts the partition back as it was if that write fails.

mod ack;
mod acquire;
mod lapse;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::sync::Arc;

use kafka_meta::{ShareBatch, ShareRecordState, ShareState};

pub(crate) use ack::{AckBatch, AckError, Acked};
#[cfg(test)]
pub(crate) use acquire::Acquired;
pub(crate) use acquire::{AcquireAsk, AcquiredRange, Aborted, Span};

/// The member a record is acquired by.
pub(crate) type Holder = Arc<str>;

/// What a share-partition is held to: the group's configs, or the broker's defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Limits {
    /// A record released (or whose lock runs out) after this many deliveries is archived (`share.delivery.count.limit`).
    pub(crate) max_delivery: i32,
    /// The most records in flight at once (`share.partition.max.record.locks`).
    pub(crate) max_in_flight: i64,
    /// How long an acquired record is locked (`share.record.lock.duration.ms`).
    pub(crate) lock_ms: i64,
    /// Whether a member may renew its locks (`share.renew.acknowledge.enable`, KIP-1222).
    pub(crate) renew: bool,
}

/// The state of an in-flight record (Kafka's `RecordState`; the DLQ's `ARCHIVING` is not here).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum RecordState {
    Available,
    Acquired,
    Acknowledged,
    Archived,
}

impl RecordState {
    /// Done with: never delivered again.
    #[must_use]
    pub(crate) fn terminal(self) -> bool {
        matches!(self, Self::Acknowledged | Self::Archived)
    }
}

/// How a transition changes the delivery count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Count {
    Increase,
    Same,
}

/// One record's state, or a whole batch's (Kafka's `InFlightState`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Rec {
    pub(crate) state: RecordState,
    pub(crate) count: i16,
    /// Who holds it, while acquired.
    pub(crate) holder: Option<Holder>,
    /// When an acquired record's lock runs out, in milliseconds since the epoch.
    pub(crate) until: Option<i64>,
}

impl Rec {
    fn new(state: RecordState, count: i16) -> Self {
        Self {
            state,
            count,
            holder: None,
            until: None,
        }
    }

    fn acquired(holder: &Holder, until: i64) -> Self {
        Self {
            state: RecordState::Acquired,
            count: 1,
            holder: Some(Arc::clone(holder)),
            until: Some(until),
        }
    }

    /// Kafka's `tryUpdateState`: the record moved to `to`, or `None` if that is not a move a record makes. A record released
    /// (or whose lock ran out) that has been delivered `max_delivery` times is archived instead.
    fn moved(&self, to: RecordState, count: Count, max_delivery: i32, holder: Option<&Holder>) -> Option<Self> {
        let to = if to == RecordState::Available && i32::from(self.count) >= max_delivery {
            RecordState::Archived
        } else {
            to
        };
        let allowed = self.state != to
            && !self.state.terminal()
            && (self.state != RecordState::Available || to == RecordState::Acquired);
        if !allowed {
            return None;
        }
        let count = match count {
            Count::Increase if to != RecordState::Archived => self.count.saturating_add(1),
            _ => self.count,
        };
        Some(Self {
            state: to,
            count,
            holder: holder.cloned(),
            until: None,
        })
    }

    /// Kafka's `archive`: archived whatever it was, its holder and lock gone.
    fn archive(&mut self) {
        self.state = RecordState::Archived;
        self.holder = None;
        self.until = None;
    }

    fn held_by(&self, member: &str) -> bool {
        self.holder.as_deref() == Some(member)
    }
}

/// A batch's state: one for all its records, or one each.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Track {
    Whole(Rec),
    Each(Vec<Rec>),
}

/// One in-flight batch (Kafka's `InFlightBatch`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InFlight {
    pub(crate) last: i64,
    pub(crate) track: Track,
}

impl InFlight {
    fn whole(last: i64, rec: Rec) -> Self {
        Self {
            last,
            track: Track::Whole(rec),
        }
    }

    fn whole_rec(&self) -> Option<&Rec> {
        match &self.track {
            Track::Whole(r) => Some(r),
            Track::Each(_) => None,
        }
    }

    fn is_each(&self) -> bool {
        matches!(self.track, Track::Each(_))
    }

    /// Kafka's `maybeInitializeOffsetStateUpdate()`: a state per offset from here on, each as the batch was.
    fn explode(&mut self, first: i64) {
        if let Track::Whole(r) = &self.track {
            let n = usize::try_from(self.last - first + 1).unwrap_or(0);
            self.track = Track::Each(vec![r.clone(); n]);
        }
    }

    /// Kafka's `maybeInitializeOffsetStateUpdate(target, delay)`: up to `target` as the batch was, past it available
    /// again, one delivery fewer.
    fn explode_up_to(&mut self, first: i64, target: i64) {
        if let Track::Whole(r) = &self.track {
            let each = (first..=self.last)
                .map(|o| {
                    if o <= target {
                        r.clone()
                    } else {
                        Rec::new(RecordState::Available, r.count.saturating_sub(1))
                    }
                })
                .collect();
            self.track = Track::Each(each);
        }
    }

    /// Every record of the batch with its offset.
    pub(crate) fn records(&self, first: i64) -> impl Iterator<Item = (i64, &Rec)> {
        let (whole, each) = match &self.track {
            Track::Whole(r) => (Some(r), None),
            Track::Each(v) => (None, Some(v)),
        };
        let whole = whole.into_iter().flat_map(move |r| (first..=self.last).map(move |o| (o, r)));
        let each = each
            .into_iter()
            .flat_map(move |v| v.iter().enumerate().map(move |(i, r)| (first + offset_of(i), r)));
        whole.chain(each)
    }
}

fn offset_of(i: usize) -> i64 {
    i64::try_from(i).unwrap_or(i64::MAX)
}

fn index_of(first: i64, offset: i64) -> usize {
    usize::try_from(offset - first).unwrap_or(usize::MAX)
}

/// The gaps of a state read back after a restart: offsets between the start and the last stored batch that no batch
/// covers (they were acquired, or never delivered) are fetched and acquired again before anything past the stored end
/// (Kafka's `GapWindow`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct GapWindow {
    end: i64,
    gap_start: i64,
}

/// One share-partition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SharePartition {
    /// The share-partition start offset (SPSO): every record before it is done with.
    pub(crate) start: i64,
    /// The share-partition end offset (SPEO): the last offset acquired so far, or the start while nothing is in flight.
    pub(crate) end: i64,
    pub(crate) cached: BTreeMap<i64, InFlight>,
    /// Records were made available again: the next fetch looks for them before the end.
    find_next: bool,
    gap: Option<GapWindow>,
    /// Kafka's state epoch: moved when the offsets are reset from outside.
    pub(crate) state_epoch: i32,
}

/// Why a stored state cannot be taken back (Kafka fails the share-partition's load).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Unreadable(pub(crate) &'static str);

impl SharePartition {
    /// A share-partition with nothing in flight, starting at `start`.
    #[must_use]
    pub(crate) fn new(start: i64, state_epoch: i32) -> Self {
        Self {
            start,
            end: start,
            cached: BTreeMap::new(),
            find_next: false,
            gap: None,
            state_epoch,
        }
    }

    /// The share-partition as it was stored (Kafka's `maybeInitialize`): what was acquired then is available now, the
    /// gaps between stored batches are fetched again.
    ///
    /// # Errors
    /// A stored batch before the start, or one that ends before it begins.
    pub(crate) fn restore(state: &ShareState) -> Result<Self, Unreadable> {
        let mut p = Self::new(state.start_offset, state.state_epoch);
        let mut gap_start = None;
        let mut previous_last = p.start - 1;
        for b in &state.batches {
            if b.first < p.start {
                return Err(Unreadable("a stored batch begins before the start offset"));
            }
            if b.last < b.first {
                return Err(Unreadable("a stored batch ends before it begins"));
            }
            if gap_start.is_none() && b.first > previous_last + 1 {
                gap_start = Some(previous_last + 1);
            }
            previous_last = b.last;
            let state = match b.state {
                ShareRecordState::Available => RecordState::Available,
                ShareRecordState::Acknowledged => RecordState::Acknowledged,
                ShareRecordState::Archived => RecordState::Archived,
            };
            p.cached
                .insert(b.first, InFlight::whole(b.last, Rec::new(state, b.delivery_count)));
        }
        if let Some((_, last)) = p.cached.last_key_value() {
            p.find_next = true;
            p.end = last.last;
            p.gap = gap_start.map(|gap_start| GapWindow {
                end: p.end,
                gap_start,
            });
            p.advance();
        }
        Ok(p)
    }

    /// What is stored: the start, and from it every record that is not simply waiting for its first delivery. A record held
    /// now is stored as it was before it was acquired (available, one delivery fewer): Kafka does not write an acquisition,
    /// so a lock never outlives the broker and a delivery that was never acknowledged is not counted after a restart.
    #[must_use]
    pub(crate) fn snapshot(&self) -> ShareState {
        let mut batches: Vec<ShareBatch> = Vec::new();
        for (&first, b) in &self.cached {
            for (offset, r) in b.records(first) {
                if offset < self.start {
                    continue;
                }
                let stored = match r.state {
                    RecordState::Available if r.count > 0 => Some((ShareRecordState::Available, r.count)),
                    RecordState::Acquired if r.count > 1 => {
                        Some((ShareRecordState::Available, r.count - 1))
                    }
                    RecordState::Acknowledged => Some((ShareRecordState::Acknowledged, r.count)),
                    RecordState::Archived => Some((ShareRecordState::Archived, r.count)),
                    RecordState::Available | RecordState::Acquired => None,
                };
                let Some((state, delivery_count)) = stored else {
                    continue;
                };
                match batches.last_mut() {
                    Some(last)
                        if last.last + 1 == offset
                            && last.state == state
                            && last.delivery_count == delivery_count =>
                    {
                        last.last = offset;
                    }
                    _ => batches.push(ShareBatch {
                        first: offset,
                        last: offset,
                        state,
                        delivery_count,
                    }),
                }
            }
        }
        ShareState {
            state_epoch: self.state_epoch,
            start_offset: self.start,
            batches,
        }
    }

    /// How many records are in flight (Kafka counts the span from the start to the end).
    #[must_use]
    pub(crate) fn in_flight(&self) -> i64 {
        if self.cached.is_empty() {
            0
        } else {
            self.end - self.start + 1
        }
    }

    /// Every record from the start on, with its offset.
    #[cfg_attr(not(feature = "inspect"), allow(dead_code))]
    pub(crate) fn records_from_start(&self) -> impl Iterator<Item = (i64, &Rec)> {
        let start = self.start;
        self.cached
            .iter()
            .flat_map(|(&first, b)| b.records(first))
            .filter(move |(o, _)| *o >= start)
    }

    /// The soonest a lock runs out, if any record is held.
    #[must_use]
    pub(crate) fn next_deadline(&self) -> Option<i64> {
        self.cached
            .iter()
            .flat_map(|(&first, b)| b.records(first))
            .filter(|(_, r)| r.state == RecordState::Acquired)
            .filter_map(|(_, r)| r.until)
            .min()
    }

    fn gap_active(&self) -> bool {
        self.gap.is_some_and(|g| g.end == self.end)
    }

    /// Kafka's `maybeUpdatePersisterGapWindowStartOffset`.
    fn update_gap_start(&mut self, offset: i64) {
        if let Some(g) = &mut self.gap {
            if g.end == self.end && offset <= g.end {
                g.gap_start = offset;
            } else {
                self.gap = None;
            }
        }
    }

    /// The offset the next read of the log starts at (Kafka's `nextFetchOffset`): the first record made available again
    /// if there is one, else the one after the end.
    pub(crate) fn next_fetch_offset(&mut self) -> i64 {
        let fresh = self
            .cached
            .last_key_value()
            .is_none_or(|(_, b)| self.start > b.last);
        if fresh {
            self.find_next = false;
            return self.end;
        }
        if !self.find_next {
            return self.end + 1;
        }
        let mut gap_start = self.gap_active().then(|| self.gap.map_or(-1, |g| g.gap_start));
        let mut next = None;
        'batches: for (&first, b) in &self.cached {
            if let Some(gs) = gap_start.as_mut() {
                if first > *gs {
                    next = Some(*gs);
                    break;
                }
                *gs = (b.last + 1).max(*gs);
            }
            match &b.track {
                Track::Whole(r) if r.state == RecordState::Available => {
                    next = Some(first);
                    break;
                }
                Track::Whole(_) => {}
                Track::Each(v) => {
                    if let Some(i) = v.iter().position(|r| r.state == RecordState::Available) {
                        next = Some(first + offset_of(i));
                        break 'batches;
                    }
                }
            }
        }
        next.unwrap_or_else(|| {
            self.find_next = false;
            self.end + 1
        })
    }

    /// Whether a fetch could acquire anything now (Kafka's `canAcquireRecords`).
    pub(crate) fn can_acquire(&mut self, limits: &Limits) -> bool {
        if self.next_fetch_offset() != self.end + 1 {
            return true;
        }
        self.in_flight() < limits.max_in_flight
    }

    /// Kafka's `canMoveStartOffset`: the record at the start is done with.
    fn can_move_start(&self) -> bool {
        let Some((&first, b)) = self.cached.range(..=self.start).next_back() else {
            return false;
        };
        let state = match &b.track {
            Track::Whole(r) => Some(r.state),
            Track::Each(v) => v.get(index_of(first, self.start)).map(|r| r.state),
        };
        state.is_some_and(RecordState::terminal)
    }

    /// Kafka's `findLastOffsetAcknowledgedAndMetadata`: the last offset of the unbroken run of records done with, from the
    /// first in flight.
    fn last_done(&self) -> Option<i64> {
        let mut last = None;
        for (&first, b) in &self.cached {
            if self.gap_active() && self.gap.is_some_and(|g| b.last >= g.gap_start) {
                return last;
            }
            match &b.track {
                Track::Whole(r) => {
                    if !r.state.terminal() {
                        return last;
                    }
                    last = Some(b.last);
                }
                Track::Each(v) => {
                    for (i, r) in v.iter().enumerate() {
                        if !r.state.terminal() {
                            return last;
                        }
                        last = Some(first + offset_of(i));
                    }
                }
            }
        }
        last
    }

    /// Moves the start past the records done with, and lets go of the batches wholly behind it (Kafka's
    /// `maybeUpdateCachedStateAndOffsets`). `true`: it moved.
    pub(crate) fn advance(&mut self) -> bool {
        if !self.can_move_start() {
            return false;
        }
        let Some(done) = self.last_done() else {
            return false;
        };
        let Some(last_cached) = self.cached.last_key_value().map(|(_, b)| b.last) else {
            return false;
        };
        if done == last_cached {
            self.start = last_cached + 1;
            self.end = last_cached + 1;
            self.cached.clear();
            return true;
        }
        let Some((&floor, floor_batch)) = self.cached.range(..=done).next_back() else {
            return false;
        };
        let first_key = self.cached.first_key_value().map_or(floor, |(k, _)| *k);
        let remove_through = if done == floor_batch.last {
            if let Some((&higher, _)) = self.cached.range(done + 1..).next()
                && higher > self.start
            {
                self.start = higher;
            }
            if self.gap_active()
                && let Some(g) = self.gap
            {
                self.start = self.start.min(g.gap_start);
            }
            Some(floor)
        } else {
            if done + 1 > self.start {
                self.start = done + 1;
            }
            if floor == first_key {
                None
            } else {
                self.cached.range(..floor).next_back().map(|(k, _)| *k)
            }
        };
        if let Some(through) = remove_through {
            let keep = self.cached.split_off(&(through + 1));
            self.cached = keep;
        }
        true
    }
}
