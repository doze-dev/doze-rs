//! Resharding: `SplitShard`, `MergeShards` and `UpdateShardCount`.
//!
//! A shard is never changed in place. It is closed at the current end of the stream (it keeps its records and gains an
//! `EndingSequenceNumber`) and children are opened to cover its hash range from that point on, so a reader drains the parent, is
//! told it has ended (no `NextShardIterator`, and `ChildShards`) and only then reads the children: that is what keeps the records
//! of one partition key in order across a reshard. The stream reports `UPDATING` for a moment afterwards and refuses another
//! reshard until it is `ACTIVE` again.
//!
//! Recorded on AWS (2026-11-09, `test_a_split_closes_the_parent_even_if_it_never_held_a_record`): the answer to a split is empty; the
//! parent is closed at once even if it never held a record; the children are `ParentShardId`-linked and split the range at
//! `NewStartingHashKey` (the lower child ends one below it). Every refusal below is worded from memory, and each says so.

use aws_wire::ApiError;
use serde_json::{Value, json};

use crate::core::{Core, shard_missing};
use crate::model::{Shard, Stream, ranges, shard_number};
use crate::req::{Req, invalid};
use crate::streams::SHARD_LIMIT;

impl Core {
    /// The shards that are open, across all streams of the region.
    fn open_shards(&self) -> i64 {
        let n: usize = self.streams.iter().map(|(_, s)| s.open().count()).sum();
        i64::try_from(n).unwrap_or(i64::MAX)
    }

    pub(crate) fn split_shard(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let (now, account, used) = (self.now_ms(), r.account().to_owned(), self.open_shards());
        let s = self.stream_mut(r)?;
        ready(s, &account, now)?;
        let id = r.s("ShardToSplit");
        let shard = find(s, id, &account)?;
        let (parent, lo, hi) = (shard.id, shard.start, shard.end);
        if shard.ended_seq.is_some() {
            // (memory)
            return Err(invalid(format!(
                "Shard {id} in stream {} under account {account} is closed, so it cannot be split.",
                s.name
            )));
        }
        let key = r.s("NewStartingHashKey");
        // The upper child starts at the key and the lower ends one below it, so the key lies after the parent's start and at or before
        // its end. (memory)
        let Some(at) = key.parse::<u128>().ok().filter(|k| *k > lo && *k <= hi) else {
            return Err(invalid(format!(
                "NewStartingHashKey {key} used in SplitShard() on shard {id} in stream {} under account {account} is not both greater than one plus the shard's StartingHashKey {lo} and less than or equal to the shard's EndingHashKey {hi}.",
                s.name
            )));
        };
        if used + 1 > SHARD_LIMIT {
            return Err(over_the_limit(&account, r.region()));
        }
        let end = s.close(parent, now);
        s.open_child((lo, at - 1), (Some(parent), None), end, now);
        s.open_child((at, hi), (Some(parent), None), end, now);
        Ok(json!({}))
    }

    pub(crate) fn merge_shards(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let (now, account) = (self.now_ms(), r.account().to_owned());
        let s = self.stream_mut(r)?;
        ready(s, &account, now)?;
        let (a_id, b_id) = (r.s("ShardToMerge"), r.s("AdjacentShardToMerge"));
        let a = find(s, a_id, &account)?;
        let b = find(s, b_id, &account)?;
        let (a, b) = (a.clone(), b.clone());
        if a.id == b.id {
            // (memory)
            return Err(invalid(format!(
                "Shard {a_id} in stream {} under account {account} cannot be merged with itself.",
                s.name
            )));
        }
        for shard in [&a, &b] {
            if shard.ended_seq.is_some() {
                // (memory)
                return Err(invalid(format!(
                    "Shard {} in stream {} under account {account} is closed, so it cannot be merged.",
                    shard.name(),
                    s.name
                )));
            }
        }
        // Merging two shards that are not neighbours would leave the hash keys between them with no shard. (memory)
        if a.end.checked_add(1) != Some(b.start) && b.end.checked_add(1) != Some(a.start) {
            return Err(invalid(format!(
                "Shards {a_id} and {b_id} in stream {} under account {account} are not an adjacent pair of shards eligible for merging.",
                s.name
            )));
        }
        let after = s.close(a.id, now).max(s.close(b.id, now));
        // The child names the shard to merge as its parent and the other as the adjacent parent. (memory)
        s.open_child(
            (a.start.min(b.start), a.end.max(b.end)),
            (Some(a.id), Some(b.id)),
            after,
            now,
        );
        Ok(json!({}))
    }

    pub(crate) fn update_shard_count(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let (now, account, used) = (self.now_ms(), r.account().to_owned(), self.open_shards());
        let target = r.int("TargetShardCount").unwrap_or(1);
        let arn = r
            .stream_name()
            .map(|n| crate::model::stream_arn(r.region(), &account, &n));
        let s = self.stream_mut(r)?;
        if s.on_demand {
            // (memory)
            return Err(ApiError::modeled(
                "ValidationException",
                format!(
                    "UpdateShardCount is not supported for stream {} under account {account}: its shard count is managed automatically in ON_DEMAND mode.",
                    s.name
                ),
            ));
        }
        ready(s, &account, now)?;
        let current = i64::try_from(s.open().count()).unwrap_or(i64::MAX);
        let out = json!({
            "StreamName": s.name,
            "CurrentShardCount": current,
            "TargetShardCount": target,
            "StreamARN": arn,
        });
        // Asking for the count the stream has changes nothing. (memory: the answer AWS gives is not recorded)
        if target == current {
            return Ok(out);
        }
        // The documented defaults: not more than double, not below half. (memory: the code and words)
        if target > current.saturating_mul(2) || target.saturating_mul(2) < current {
            return Err(ApiError::modeled(
                "LimitExceededException",
                format!(
                    "Cannot update the shard count of stream {} under account {account} from {current} to {target}: the new count may not be more than double or less than half the current one.",
                    s.name
                ),
            ));
        }
        if used - current + target > SHARD_LIMIT {
            return Err(over_the_limit(&account, r.region()));
        }
        // Uniform scaling: every open shard is closed and the hash space is cut again into equal ranges. A child names the shard that
        // owned its first hash key and, when it spans two, the one that owned its last: doubling splits each shard in two and
        // halving merges pairs, which is exactly what this gives.
        let old: Vec<(u32, u128, u128)> = s.open().map(|x| (x.id, x.start, x.end)).collect();
        let mut after = 0;
        for (id, _, _) in &old {
            after = after.max(s.close(*id, now));
        }
        let owner = |hash: u128| {
            old.iter()
                .find(|(_, lo, hi)| (*lo..=*hi).contains(&hash))
                .map(|o| o.0)
        };
        for (lo, hi) in ranges(u32::try_from(target).unwrap_or(1)) {
            let first = owner(lo);
            let last = owner(hi).filter(|l| Some(*l) != first);
            s.open_child((lo, hi), (first, last), after, now);
        }
        Ok(out)
    }
}

/// A reshard needs the stream `ACTIVE`: not still moving from the last one. (memory)
fn ready(s: &Stream, account: &str, now_ms: i64) -> Result<(), ApiError> {
    match s.status(now_ms) {
        "ACTIVE" => Ok(()),
        state => Err(ApiError::modeled(
            "ResourceInUseException",
            format!(
                "Stream {} under account {account} not ACTIVE, instead in state {state}",
                s.name
            ),
        )),
    }
}

/// The shard named `id`, or the not-found answer.
fn find<'a>(s: &'a Stream, id: &str, account: &str) -> Result<&'a Shard, ApiError> {
    shard_number(id)
        .and_then(|n| s.shard(n))
        .ok_or_else(|| shard_missing(id, &s.name, account))
}

/// (memory) The same wording as `CreateStream`'s.
fn over_the_limit(account: &str, region: &str) -> ApiError {
    ApiError::modeled(
        "LimitExceededException",
        format!("This request would exceed the shard limit for the account {account} in {region}."),
    )
}
