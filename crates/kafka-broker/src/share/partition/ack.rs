//! Acknowledging what a member holds, and letting go of what it holds when it goes (Kafka's `SharePartition.acknowledge`
//! and `releaseAcquiredRecords`). A request that fails part way changes nothing: the caller puts back the share-partition
//! it had before (Kafka rolls back the transitions it started).

use kafka_wire::ResponseError;

use super::{Count, Limits, Rec, RecordState, SharePartition, Track, index_of, offset_of};

/// One acknowledgement batch of a request: a type for the whole range, or one per offset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AckBatch {
    pub(crate) first: i64,
    pub(crate) last: i64,
    /// 0 gap, 1 accept, 2 release, 3 reject, 4 renew.
    pub(crate) types: Vec<i8>,
}

/// Why an acknowledgement was refused, in Kafka's words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AckError {
    pub(crate) code: ResponseError,
    pub(crate) message: String,
}

pub(crate) const GAP: i8 = 0;
pub(crate) const ACCEPT: i8 = 1;
pub(crate) const RELEASE: i8 = 2;
pub(crate) const REJECT: i8 = 3;
pub(crate) const RENEW: i8 = 4;

fn bad_state(message: &str) -> AckError {
    AckError {
        code: ResponseError::InvalidRecordState,
        message: message.to_owned(),
    }
}

fn bad_request(message: impl Into<String>) -> AckError {
    AckError {
        code: ResponseError::InvalidRequest,
        message: message.into(),
    }
}

/// What an acknowledge type moves a record to (Kafka's `ACK_TYPE_TO_RECORD_STATE`); renew moves nothing.
fn target(ack: i8) -> Option<RecordState> {
    match ack {
        GAP | REJECT => Some(RecordState::Archived),
        ACCEPT => Some(RecordState::Acknowledged),
        RELEASE => Some(RecordState::Available),
        _ => None,
    }
}

/// Kafka's `fetchAckTypeMapForBatch` check: every type but a gap must be one Kafka knows.
fn check_types(batch: &AckBatch) -> Result<(), AckError> {
    if batch.types.iter().all(|t| (GAP..=RENEW).contains(t)) {
        return Ok(());
    }
    let listed: Vec<String> = batch.types.iter().map(i8::to_string).collect();
    Err(bad_request(format!(
        "Invalid acknowledge type: [{}]",
        listed.join(", ")
    )))
}

/// What came of an acknowledgement.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Acked {
    /// Some record changed state: the change is to be stored.
    pub(crate) changed: bool,
    /// Some record was made available again: a waiting fetch may have something now.
    pub(crate) released: bool,
}

/// Who acknowledges, and when, under which limits.
struct Acking<'a> {
    member: &'a str,
    now: i64,
    limits: &'a Limits,
}

impl SharePartition {
    /// Acknowledges `batches` for `member` at `now`. On an error the share-partition may be half changed: the caller puts
    /// back the one it had. The start does not move here: the caller writes the change first, then calls
    /// [`SharePartition::advance`] (Kafka writes the start offset it had before the write, and moves it after).
    ///
    /// # Errors
    /// INVALID_REQUEST or INVALID_RECORD_STATE, with Kafka's message.
    pub(crate) fn acknowledge(
        &mut self,
        member: &str,
        batches: &[AckBatch],
        now: i64,
        limits: &Limits,
    ) -> Result<Acked, AckError> {
        let who = Acking {
            member,
            now,
            limits,
        };
        let mut acked = Acked::default();
        for batch in batches {
            check_types(batch)?;
            if batch.last < self.start {
                continue;
            }
            let keys = self.ack_keys(batch)?;
            for key in keys {
                self.ack_in_batch(&who, batch, key, &mut acked)?;
            }
        }
        if acked.released {
            self.find_next = true;
        }
        Ok(acked)
    }

    /// Kafka's `fetchSubMapForAcknowledgementBatch`: the batches in flight the acknowledgement covers.
    fn ack_keys(&self, batch: &AckBatch) -> Result<Vec<i64>, AckError> {
        let floor = match self.cached.range(..=batch.first).next_back() {
            Some((&k, _)) => k,
            None if batch.first < self.start && batch.last >= self.start => self
                .cached
                .range(..=self.start)
                .next_back()
                .map(|(k, _)| *k)
                .ok_or_else(|| bad_state(NOT_FOUND))?,
            None => return Err(bad_state(NOT_FOUND)),
        };
        let keys: Vec<i64> = self
            .cached
            .range(floor..=batch.last)
            .map(|(k, _)| *k)
            .collect();
        let last = keys
            .last()
            .and_then(|k| self.cached.get(k))
            .map_or(i64::MIN, |b| b.last);
        if last < batch.first {
            return Err(bad_request(
                "Batch record not found. The first offset in request is past acquired records.",
            ));
        }
        if batch.last > last {
            return Err(bad_request(
                "Batch record not found. The last offset in request is past acquired records.",
            ));
        }
        Ok(keys)
    }

    /// Kafka's `acknowledgeBatchRecords`, for one batch in flight.
    fn ack_in_batch(
        &mut self,
        who: &Acking<'_>,
        batch: &AckBatch,
        key: i64,
        acked: &mut Acked,
    ) -> Result<(), AckError> {
        let (member, now, limits) = (who.member, who.now, who.limits);
        let start = self.start;
        let Some(b) = self.cached.get_mut(&key) else {
            return Ok(());
        };
        if b.last < start {
            return Ok(());
        }
        if let Track::Whole(r) = &b.track {
            match &r.holder {
                None => {
                    return Err(bad_state(
                        "The batch cannot be acknowledged. The batch is not in the acquired state.",
                    ));
                }
                Some(h) if &**h != member => {
                    return Err(bad_state("Member is not the owner of batch record"));
                }
                Some(_) => {}
            }
        }
        let full = key >= batch.first && b.last <= batch.last;
        let per_offset = batch.types.len() > 1;
        let start_within = key < start && b.last >= start;
        if !full || b.is_each() || per_offset || start_within {
            if b.whole_rec()
                .is_some_and(|r| r.state != RecordState::Acquired)
            {
                return Err(bad_state(
                    "The batch cannot be acknowledged. The subset batch is not in the acquired state.",
                ));
            }
            b.explode(key);
            let Track::Each(records) = &mut b.track else {
                return Ok(());
            };
            for (i, r) in records.iter_mut().enumerate() {
                let offset = key + offset_of(i);
                if offset < batch.first || offset < start {
                    continue;
                }
                if offset > batch.last {
                    break;
                }
                if r.state != RecordState::Acquired {
                    return Err(bad_state(
                        "The offset cannot be acknowledged. The offset is not acquired.",
                    ));
                }
                if !r.held_by(member) {
                    return Err(bad_state("Member is not the owner of offset"));
                }
                let ack = if per_offset {
                    batch.types.get(index_of(batch.first, offset)).copied()
                } else {
                    batch.types.first().copied()
                };
                apply(r, ack, now, limits, acked)?;
            }
            return Ok(());
        }
        let Track::Whole(r) = &mut b.track else {
            return Ok(());
        };
        if r.state != RecordState::Acquired {
            return Err(bad_state(
                "The batch cannot be acknowledged. The batch is not in the acquired state.",
            ));
        }
        apply(r, batch.types.first().copied(), now, limits, acked)
    }

    /// Lets go of what `member` holds (its share session closed, or it left the group): available again, or archived if
    /// it has been delivered as often as allowed or is behind the start. Kafka's `releaseAcquiredRecords`.
    pub(crate) fn release_member(&mut self, member: &str, limits: &Limits) -> Acked {
        let start = self.start;
        let mut acked = Acked::default();
        for (&key, b) in &mut self.cached {
            if let Track::Whole(r) = &b.track
                && r.state == RecordState::Acquired
                && r.held_by(member)
                && key < start
                && b.last >= start
            {
                b.explode(key);
            }
            match &mut b.track {
                Track::Whole(r) => {
                    if r.holder.as_deref().is_some_and(|h| h != member)
                        || r.state != RecordState::Acquired
                    {
                        continue;
                    }
                    let to = if b.last < start {
                        RecordState::Archived
                    } else {
                        RecordState::Available
                    };
                    release(r, to, limits, &mut acked);
                }
                Track::Each(records) => {
                    for (i, r) in records.iter_mut().enumerate() {
                        if r.holder.as_deref().is_some_and(|h| h != member)
                            || r.state != RecordState::Acquired
                        {
                            continue;
                        }
                        let to = if key + offset_of(i) < start {
                            RecordState::Archived
                        } else {
                            RecordState::Available
                        };
                        release(r, to, limits, &mut acked);
                    }
                }
            }
        }
        if acked.released {
            self.find_next = true;
        }
        acked
    }
}

const NOT_FOUND: &str =
    "Batch record not found. The request batch offsets are not found in the cache.";

/// One record (or a whole batch) acknowledged with `ack`.
fn apply(
    r: &mut Rec,
    ack: Option<i8>,
    now: i64,
    limits: &Limits,
    acked: &mut Acked,
) -> Result<(), AckError> {
    let Some(ack) = ack else {
        return Err(bad_request("Invalid acknowledge type: []"));
    };
    if ack == RENEW {
        if !limits.renew {
            return Err(bad_state(
                "Renewing acquisition locks is not enabled for the group.",
            ));
        }
        r.until = Some(now + limits.lock_ms);
        return Ok(());
    }
    let to =
        target(ack).ok_or_else(|| bad_request(format!("Unknown acknowledge type id: {ack}")))?;
    let next = r
        .moved(to, Count::Same, limits.max_delivery, None)
        .ok_or_else(|| bad_state("Unable to acknowledge records for the batch"))?;
    acked.changed = true;
    acked.released |= next.state == RecordState::Available;
    *r = next;
    Ok(())
}

fn release(r: &mut Rec, to: RecordState, limits: &Limits, acked: &mut Acked) {
    if let Some(next) = r.moved(to, Count::Same, limits.max_delivery, None) {
        acked.changed = true;
        acked.released |= next.state == RecordState::Available;
        *r = next;
    }
}

impl Track {
    /// The records the batch holds for `member`, for tests and the console.
    #[cfg(test)]
    pub(crate) fn held_by(&self, member: &str) -> usize {
        match self {
            Track::Whole(r) => usize::from(r.held_by(member)),
            Track::Each(v) => v.iter().filter(|r| r.held_by(member)).count(),
        }
    }
}
