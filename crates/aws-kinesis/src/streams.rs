//! Streams: create, delete, describe, list.

use std::collections::BTreeSet;

use aws_wire::ApiError;
use serde_json::{Value, json};

use crate::core::{Core, new_stream_id};
use crate::model::{Shard, Stream, ranges};
use crate::req::{Put, Req, invalid, secs};

/// Shards one region may hold (AWS's default limit is 500 in the large regions; the limit is a setting there). (memory)
pub(crate) const SHARD_LIMIT: i64 = 500;
/// Shards of an on-demand stream when it is made.
const ON_DEMAND_SHARDS: u32 = 4;

impl Core {
    pub(crate) fn create_stream(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = r.s("StreamName").to_owned();
        let account = r.account().to_owned();
        if self.streams.get(&name).is_some() {
            return Err(ApiError::modeled(
                "ResourceInUseException",
                format!("Stream {name} under account {account} already exists."),
            ));
        }
        let on_demand =
            r.v.pointer("/StreamModeDetails/StreamMode")
                .and_then(Value::as_str)
                == Some("ON_DEMAND");
        let count = match (on_demand, r.int("ShardCount")) {
            (true, _) => i64::from(ON_DEMAND_SHARDS),
            (false, Some(n)) => n,
            // (memory)
            (false, None) => {
                return Err(invalid(
                    "ShardCount must be specified for a PROVISIONED stream.",
                ));
            }
        };
        let used: usize = self.streams.iter().map(|(_, s)| s.open().count()).sum();
        if i64::try_from(used).unwrap_or(i64::MAX) + count > SHARD_LIMIT {
            // (memory)
            return Err(ApiError::modeled(
                "LimitExceededException",
                format!(
                    "This request would exceed the shard limit for the account {account} in {}.",
                    r.region()
                ),
            ));
        }
        let now = self.now_ms();
        let mut stream = Stream {
            no: self
                .streams
                .iter()
                .map(|(_, s)| s.no)
                .max()
                .map_or(1, |n| n + 1),
            name: name.clone(),
            stream_id: new_stream_id(),
            created_ms: now,
            on_demand,
            shards: Vec::new(),
            next_shard: 0,
            retention_hours: 24,
            kms_key: None,
            tags: r.map("Tags").into_iter().collect(),
            metrics: BTreeSet::new(),
            consumers: Vec::new(),
            policy: None,
            max_record_kib: r.int("MaxRecordSizeInKiB").unwrap_or(1024),
            warm_mibps: r.int("WarmThroughputMiBps"),
            distribution: r.opt("RecordDistributionStrategy").map(str::to_owned),
        };
        open_shards(&mut stream, count, now);
        self.streams.insert(name, stream);
        Ok(json!({}))
    }

    pub(crate) fn delete_stream(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let stream = self.stream(r)?;
        if !stream.consumers.is_empty() && r.flag("EnforceConsumerDeletion") != Some(true) {
            // (memory)
            return Err(ApiError::modeled(
                "ResourceInUseException",
                format!(
                    "Stream {} under account {} has consumers registered. Use EnforceConsumerDeletion to delete them.",
                    stream.name,
                    r.account()
                ),
            ));
        }
        let (name, no) = (stream.name.clone(), stream.no);
        self.streams.remove(&name);
        self.trimmed.remove(&name);
        self.rows
            .delete_range(&format!("{no:08x}"), &format!("{:08x}", no + 1));
        Ok(json!({}))
    }

    pub(crate) fn describe_stream(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let s = self.stream(r)?;
        let limit = r
            .int("Limit")
            .map_or(100, |n| usize::try_from(n).unwrap_or(100));
        let after = r
            .opt("ExclusiveStartShardId")
            .and_then(crate::model::shard_number);
        let rest: Vec<&Shard> = s
            .shards
            .iter()
            .filter(|sh| after.is_none_or(|a| sh.id > a))
            .collect();
        let shown: Vec<Value> = rest.iter().take(limit).map(|sh| shard_json(sh)).collect();
        let mut d = common(s, r.region(), r.account(), self.now_ms());
        d.put("Shards", Value::Array(shown));
        d.put("HasMoreShards", json!(rest.len() > limit));
        Ok(json!({ "StreamDescription": d }))
    }

    pub(crate) fn describe_stream_summary(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let s = self.stream(r)?;
        let mut d = common(s, r.region(), r.account(), self.now_ms());
        d.put("OpenShardCount", json!(s.open().count()));
        d.put("ConsumerCount", json!(s.consumers.len()));
        d.put("MaxRecordSizeInKiB", json!(s.max_record_kib));
        d.put("ChannelCount", json!(0));
        if let Some(w) = s.warm_mibps {
            d.put(
                "WarmThroughput",
                json!({"TargetMiBps": w, "CurrentMiBps": w}),
            );
        }
        if let Some(x) = &s.distribution {
            d.put("RecordDistributionStrategy", json!(x));
        }
        Ok(json!({ "StreamDescriptionSummary": d }))
    }

    pub(crate) fn list_streams(&self, r: &Req<'_>) -> Value {
        let now = self.now_ms();
        let limit = r
            .int("Limit")
            .map_or(100, |n| usize::try_from(n).unwrap_or(100));
        let after = r
            .opt("ExclusiveStartStreamName")
            .or_else(|| r.opt("NextToken"))
            .unwrap_or_default();
        let rest: Vec<&Stream> = self
            .streams
            .iter()
            .map(|(_, s)| s)
            .filter(|s| after.is_empty() || s.name.as_str() > after)
            .collect();
        let shown: Vec<&Stream> = rest.iter().take(limit).copied().collect();
        let more = rest.len() > limit;
        let mut out = json!({
            "StreamNames": shown.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
            "HasMoreStreams": more,
            "StreamSummaries": shown.iter().map(|s| json!({
                "StreamName": s.name,
                "StreamARN": s.arn(r.region(), r.account()),
                "StreamStatus": s.status(now),
                "StreamModeDetails": mode_of(s),
                "StreamCreationTimestamp": secs(s.created_ms),
            })).collect::<Vec<_>>(),
        });
        if let (true, Some(last)) = (more, shown.last()) {
            out.put("NextToken", json!(last.name));
        }
        out
    }

    pub(crate) fn describe_limits(&self) -> Value {
        let open: usize = self.streams.iter().map(|(_, s)| s.open().count()).sum();
        let on_demand = self.streams.iter().filter(|(_, s)| s.on_demand).count();
        json!({
            "ShardLimit": SHARD_LIMIT,
            "OpenShardCount": open,
            "OnDemandStreamCount": on_demand,
            "OnDemandStreamCountLimit": 50,
        })
    }

    pub(crate) fn update_mode(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let on_demand =
            r.v.pointer("/StreamModeDetails/StreamMode")
                .and_then(Value::as_str)
                == Some("ON_DEMAND");
        self.stream_mut(r)?.on_demand = on_demand;
        Ok(json!({}))
    }
}

/// Gives a new stream `count` shards that divide the hash space evenly. Each starts at a sequence number of its own (recorded on
/// AWS, 2026-11-09).
fn open_shards(stream: &mut Stream, count: i64, now_ms: i64) {
    let first_seq = u128::try_from(now_ms).unwrap_or(0) << crate::model::SEQ_SHIFT;
    for (start, end) in ranges(u32::try_from(count).unwrap_or(1)) {
        stream.shards.push(Shard {
            id: stream.next_shard,
            start,
            end,
            parent: None,
            adjacent_parent: None,
            first_seq: first_seq + u128::from(stream.next_shard),
            last_seq: 0,
            ended_seq: None,
        });
        stream.next_shard += 1;
    }
}

pub(crate) fn mode_of(s: &Stream) -> Value {
    json!({"StreamMode": if s.on_demand { "ON_DEMAND" } else { "PROVISIONED" }})
}

/// The members a description and a summary share.
fn common(s: &Stream, region: &str, account: &str, now_ms: i64) -> Value {
    let metrics: Vec<&String> = s.metrics.iter().collect();
    let mut d = json!({
        "StreamName": s.name,
        "StreamARN": s.arn(region, account),
        "StreamStatus": s.status(now_ms),
        "StreamModeDetails": mode_of(s),
        "RetentionPeriodHours": s.retention_hours,
        "StreamCreationTimestamp": secs(s.created_ms),
        "EnhancedMonitoring": [{"ShardLevelMetrics": metrics}],
        "EncryptionType": if s.kms_key.is_some() { "KMS" } else { "NONE" },
    });
    if let Some(k) = &s.kms_key {
        d.put("KeyId", json!(k));
    }
    d
}

/// The `ChildShards` of a closed shard, as `GetRecords` shows them at the end of it.
pub(crate) fn child_shards_json(s: &Stream, id: u32) -> Vec<Value> {
    s.children(id)
        .map(|c| {
            let parents: Vec<String> = [c.parent, c.adjacent_parent]
                .into_iter()
                .flatten()
                .map(crate::model::shard_name)
                .collect();
            json!({
                "ShardId": c.name(),
                "ParentShards": parents,
                "HashKeyRange": {
                    "StartingHashKey": c.start.to_string(),
                    "EndingHashKey": c.end.to_string(),
                },
            })
        })
        .collect()
}

/// A shard as `ListShards` and `DescribeStream` show it.
pub(crate) fn shard_json(sh: &Shard) -> Value {
    let mut range = json!({"StartingSequenceNumber": sh.first_seq.to_string()});
    if let Some(e) = sh.ended_seq {
        range.put("EndingSequenceNumber", json!(e.to_string()));
    }
    let mut v = json!({
        "ShardId": sh.name(),
        "HashKeyRange": {
            "StartingHashKey": sh.start.to_string(),
            "EndingHashKey": sh.end.to_string(),
        },
        "SequenceNumberRange": range,
    });
    if let Some(p) = sh.parent {
        v.put("ParentShardId", json!(crate::model::shard_name(p)));
    }
    if let Some(p) = sh.adjacent_parent {
        v.put("AdjacentParentShardId", json!(crate::model::shard_name(p)));
    }
    v
}
