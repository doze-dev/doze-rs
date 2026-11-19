//! Enhanced fan-out reads, the actor's half of `SubscribeToShard`: the subscription is checked and its starting position worked out
//! here ([`Core::open_subscription`]), and each read of the shard it is pushed is made here ([`Core::fanout_read`]). The stream of
//! events itself, the five-minute life and the one-subscription-per-consumer-and-shard rule are `subscribe.rs`.
//!
//! As the model documents `SubscribeToShard`: a registered consumer (`ConsumerARN`, from `RegisterStreamConsumer`), a shard, and a
//! `StartingPosition` (`AT_SEQUENCE_NUMBER`, `AFTER_SEQUENCE_NUMBER`, `AT_TIMESTAMP`, `TRIM_HORIZON`, `LATEST`). Each event carries
//! the records read, a `ContinuationSequenceNumber` to resume from "even when no data is written to the shard", `MillisBehindLatest`,
//! and, at the end of a closed shard, its `ChildShards`.

use aws_wire::ApiError;
use serde_json::{Value, json};
use tokio::sync::oneshot;

use crate::core::{Core, shard_missing};
use crate::model::{Stream, shard_number};
use crate::reading::{millis_between, record_json, seq_of_time};
use crate::req::{Put, Req, invalid, name_of_arn};

/// What a subscription reads: the consumer it is for, and where in which shard it is.
#[derive(Clone, Debug)]
pub(crate) struct Position {
    pub(crate) consumer_arn: String,
    pub(crate) stream: String,
    pub(crate) shard: u32,
    pub(crate) next: u128,
}

/// One event's worth of a shard: the `SubscribeToShardEvent` body, where to read on from, and whether the shard has ended.
#[derive(Debug)]
pub(crate) struct Batch {
    pub(crate) event: Value,
    pub(crate) next: u128,
    pub(crate) records: usize,
    pub(crate) ended: bool,
}

/// What the actor is asked on behalf of a subscription.
#[derive(Debug)]
pub(crate) enum FanoutAsk {
    /// A `SubscribeToShard` call to check: the position it starts at, or its refusal.
    Open(
        Box<aws_wire::service::Call>,
        oneshot::Sender<Result<Position, ApiError>>,
    ),
    /// The next read of an open subscription, made for the account that subscribed.
    Read(Position, String, oneshot::Sender<Result<Batch, ApiError>>),
}

/// The consumer's stream name, name and creation second, from `arn:aws:kinesis:r:a:stream/S/consumer/C:T`.
fn parts_of(arn: &str) -> Option<(String, &str, i64)> {
    let stream = name_of_arn(arn)?;
    let (name, at) = arn.split_once("/consumer/")?.1.rsplit_once(':')?;
    Some((stream, name, at.parse().ok()?))
}

/// (memory) The consumer is gone, or never was.
fn no_consumer(arn: &str) -> ApiError {
    ApiError::modeled(
        "ResourceNotFoundException",
        format!("Consumer {arn} not found."),
    )
}

impl Core {
    /// The stream a consumer ARN names, when the consumer is registered on it (the creation time in the ARN tells a consumer from
    /// an earlier one of the same name).
    fn consumer_of(&self, arn: &str, account: &str) -> Result<&Stream, ApiError> {
        let (stream, name, at) = parts_of(arn).ok_or_else(|| no_consumer(arn))?;
        let s = self
            .streams
            .get(&stream)
            .ok_or_else(|| crate::core::not_found(&stream, account))?;
        s.consumers
            .iter()
            .any(|c| c.name == name && c.created_ms / 1000 == at)
            .then_some(s)
            .ok_or_else(|| no_consumer(arn))
    }

    /// Checks a `SubscribeToShard` call and works out the sequence number it starts reading at.
    pub(crate) fn open_subscription(&mut self, r: &Req<'_>) -> Result<Position, ApiError> {
        let arn = r.s("ConsumerARN").to_owned();
        let s = self.consumer_of(&arn, r.account())?;
        let id = r.s("ShardId");
        let shard = shard_number(id)
            .and_then(|n| s.shard(n))
            .ok_or_else(|| shard_missing(id, &s.name, r.account()))?;
        let pos = r.v.get("StartingPosition").cloned().unwrap_or(Value::Null);
        let kind = pos.get("Type").and_then(Value::as_str).unwrap_or_default();
        let number = pos.get("SequenceNumber").and_then(Value::as_str);
        let parsed = number.map(str::parse::<u128>);
        let next = match (kind, parsed) {
            ("AT_SEQUENCE_NUMBER", Some(Ok(n))) => n,
            ("AFTER_SEQUENCE_NUMBER", Some(Ok(n))) => n.saturating_add(1),
            ("TRIM_HORIZON", None) => 0,
            ("LATEST", None) => shard.last_seq.saturating_add(1),
            ("AT_TIMESTAMP", None) => seq_of_time(pos.get("Timestamp")).ok_or_else(|| {
                // (memory)
                invalid("Timestamp must be specified when StartingPosition Type is AT_TIMESTAMP.")
            })?,
            (kind, _) => {
                // (memory) worded after GetShardIterator's recorded refusal.
                return Err(invalid(format!(
                    "StartingPosition Type {kind} {} a SequenceNumber.",
                    if number.is_some() {
                        "does not take"
                    } else {
                        "needs"
                    }
                )));
            }
        };
        let position = Position {
            consumer_arn: arn,
            stream: s.name.clone(),
            shard: shard.id,
            next,
        };
        if r.flag("DryRun") == Some(true) {
            // (memory) The model lists `DryRunOperationException` for a request that would have succeeded.
            return Err(ApiError::modeled(
                "DryRunOperationException",
                "The request would have succeeded, but the DryRun flag is set.",
            ));
        }
        Ok(position)
    }

    /// The next event of a subscription: what the shard holds from its position on.
    pub(crate) fn fanout_read(&mut self, p: &Position, account: &str) -> Result<Batch, ApiError> {
        let (no, tip, closed, kms) = {
            let s = self.consumer_of(&p.consumer_arn, account)?;
            let sh = s.shard(p.shard).ok_or_else(|| {
                shard_missing(&crate::model::shard_name(p.shard), &s.name, account)
            })?;
            (
                s.no,
                sh.last_seq,
                sh.ended_seq.is_some(),
                s.kms_key.is_some(),
            )
        };
        self.trim(&p.stream);
        let (found, next, exhausted) = self.read_shard((no, &p.stream), p.shard, p.next)?;
        let records: Vec<Value> = found
            .iter()
            .map(|(seq, rec)| record_json(*seq, rec, kms))
            .collect();
        let last = found.last().map(|(seq, _)| *seq);
        let behind = last.map_or(0, |seq| millis_between(seq, tip));
        let mut event = json!({"Records": records, "MillisBehindLatest": behind});
        let ended = closed && exhausted;
        if ended {
            // (memory) At the end of a closed shard there is nothing to continue from: the KCL reads a missing continuation with
            // `ChildShards` as the shard's end.
            let children = self
                .streams
                .get(&p.stream)
                .map(|s| crate::streams::child_shards_json(s, p.shard))
                .unwrap_or_default();
            event.put("ChildShards", json!(children));
        } else {
            // The last record read, or (nothing read) the number just before where the next read starts, so that
            // `AFTER_SEQUENCE_NUMBER` resumes at the same place.
            let at = last.unwrap_or_else(|| next.saturating_sub(1));
            event.put("ContinuationSequenceNumber", json!(at.to_string()));
        }
        Ok(Batch {
            event,
            next,
            records: found.len(),
            ended,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::parts_of;

    #[test]
    fn a_consumer_arn_names_its_stream_consumer_and_second() {
        assert_eq!(
            parts_of("arn:aws:kinesis:us-east-1:1:stream/s/consumer/c:1700000000"),
            Some(("s".to_owned(), "c", 1_700_000_000))
        );
        assert_eq!(parts_of("arn:aws:kinesis:us-east-1:1:stream/s"), None);
    }
}
