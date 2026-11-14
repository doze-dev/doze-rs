//! DynamoDB Streams (`DynamoDBStreams_20120810`): `ListStreams`, `DescribeStream`, `GetShardIterator` and `GetRecords`, read from
//! the records `aws-dynamodb` keeps for every table with a stream. It holds no state of its own: a shard iterator carries the stream
//! and the position in it.
//!
//! A stream here has one shard, open while the stream is enabled; its id has the form AWS gives. Sequence numbers are the ones the
//! table's stream records carry (21 digits, increasing). DynamoDB and DynamoDB Streams both sign as `dynamodb`: the gateway tells them
//! apart by the `X-Amz-Target` prefix.
//!
//! Wording: every message here is `(memory)`, from the API reference and from use, unchecked until a recording reaches it.

#![forbid(unsafe_code)]

use std::sync::{Arc, OnceLock};

use aws_dynamodb::{Dynamodb, StreamInfo};
use aws_model_dynamodb_streams::Operation;
use aws_model_dynamodb_streams::meta::META;
use aws_model_dynamodb_streams::tables::TABLES;
use aws_peers::{Need, Peers};
use aws_schema::patterns::Patterns;
use aws_wire::ApiError;
use aws_wire::handler::reply_json;
use aws_wire::service::{BoxFuture, Call, Caller, Reply, Service};
use serde_json::{Value as Json, json};

/// How long a shard iterator may be used, as on AWS.
const ITERATOR_MS: i64 = 15 * 60 * 1000;
/// The most records one `GetRecords` returns, and its default.
const MAX_RECORDS: i64 = 1000;
/// The most streams one `ListStreams` returns, and its default.
const MAX_STREAMS: i64 = 100;

/// The service, over a DynamoDB that keeps the records.
#[derive(Debug)]
pub struct Streams {
    ddb: Arc<Dynamodb>,
    peers: Option<Arc<Peers>>,
}

impl Streams {
    /// Streams over `ddb`. `peers` authorizes the caller when IAM is on.
    #[must_use]
    pub fn new(ddb: Arc<Dynamodb>, peers: Option<Arc<Peers>>) -> Self {
        Self { ddb, peers }
    }

    async fn ask(&self, call: Call) -> Result<Json, ApiError> {
        let op = Operation::from_index(call.op)
            .ok_or_else(|| ApiError::internal("unknown operation index"))?;
        let body: Json = serde_json::from_str(if call.body.trim().is_empty() {
            "{}"
        } else {
            &call.body
        })
        .map_err(|_| validation("Unable to parse the request body."))?;
        let caller = &call.caller;
        match op {
            Operation::ListStreams => {
                self.allow(caller, "ListStreams", stream_arn_any(caller))
                    .await?;
                self.list(caller, &body).await
            }
            Operation::DescribeStream => {
                let arn = text(&body, "StreamArn");
                self.allow(caller, "DescribeStream", arn.to_owned()).await?;
                self.describe(caller, arn, &body).await
            }
            Operation::GetShardIterator => {
                let arn = text(&body, "StreamArn");
                self.allow(caller, "GetShardIterator", arn.to_owned())
                    .await?;
                self.iterator(caller, arn, &body).await
            }
            Operation::GetRecords => {
                let it = Iterator::read(text(&body, "ShardIterator"))?;
                self.allow(caller, "GetRecords", it.arn.clone()).await?;
                self.records(caller, it, &body).await
            }
        }
    }

    async fn allow(&self, caller: &Caller, action: &str, resource: String) -> Result<(), ApiError> {
        match &self.peers {
            Some(p) => {
                p.authorize(
                    caller,
                    vec![Need::new(format!("dynamodb:{action}"), resource)],
                )
                .await
            }
            None => Ok(()),
        }
    }

    /// The stream an ARN names, if it is the current stream of a table in the caller's region.
    async fn stream(&self, caller: &Caller, arn: &str) -> Result<StreamInfo, ApiError> {
        let table = arn
            .split_once(":table/")
            .and_then(|(_, rest)| rest.split_once("/stream/"))
            .map(|(t, _)| t)
            .ok_or_else(|| stream_not_found(arn))?;
        self.ddb
            .streams(&caller.region, &caller.account_id, Some(table))
            .await
            .map_err(ApiError::internal)?
            .into_iter()
            .find(|s| s.arn == arn)
            .ok_or_else(|| stream_not_found(arn))
    }

    async fn list(&self, caller: &Caller, body: &Json) -> Result<Json, ApiError> {
        let limit = limit(body, MAX_STREAMS)?;
        let all = self
            .ddb
            .streams(
                &caller.region,
                &caller.account_id,
                body.get("TableName").and_then(Json::as_str),
            )
            .await
            .map_err(ApiError::internal)?;
        let after = body.get("ExclusiveStartStreamArn").and_then(Json::as_str);
        let start = after.map_or(0, |a| {
            all.iter()
                .position(|s| s.arn == a)
                .map_or(all.len(), |i| i + 1)
        });
        let page: Vec<&StreamInfo> = all.iter().skip(start).take(limit).collect();
        let streams: Vec<Json> = page
            .iter()
            .map(|s| json!({"StreamArn": s.arn, "TableName": s.table, "StreamLabel": s.label}))
            .collect();
        let mut out = json!({ "Streams": streams });
        if start + page.len() < all.len()
            && let Some(last) = page.last()
        {
            put(&mut out, "LastEvaluatedStreamArn", json!(last.arn));
        }
        Ok(out)
    }

    async fn describe(&self, caller: &Caller, arn: &str, body: &Json) -> Result<Json, ApiError> {
        limit(body, 100)?;
        let s = self.stream(caller, arn).await?;
        let mut range = json!({"StartingSequenceNumber": sequence_number(1)});
        if !s.enabled {
            put(
                &mut range,
                "EndingSequenceNumber",
                json!(sequence_number(s.last_seq.max(1))),
            );
        }
        // One shard: it is past an `ExclusiveStartShardId` that names it, and it has no children.
        let past =
            body.get("ExclusiveStartShardId").and_then(Json::as_str) == Some(s.shard_id.as_str());
        let children =
            body.pointer("/ShardFilter/Type").and_then(Json::as_str) == Some("CHILD_SHARDS");
        let shards = if past || children {
            json!([])
        } else {
            json!([{"ShardId": s.shard_id, "SequenceNumberRange": range}])
        };
        let keys: Vec<Json> = s
            .key_schema
            .iter()
            .map(|(name, kind)| json!({"AttributeName": name, "KeyType": kind}))
            .collect();
        Ok(json!({"StreamDescription": {
            "StreamArn": s.arn,
            "StreamLabel": s.label,
            "StreamStatus": if s.enabled { "ENABLED" } else { "DISABLED" },
            "StreamViewType": s.view,
            "CreationRequestDateTime": seconds(s.created_ms),
            "TableName": s.table,
            "KeySchema": keys,
            "Shards": shards,
        }}))
    }

    async fn iterator(&self, caller: &Caller, arn: &str, body: &Json) -> Result<Json, ApiError> {
        let s = self.stream(caller, arn).await?;
        if text(body, "ShardId") != s.shard_id {
            return Err(ApiError::modeled(
                "ResourceNotFoundException",
                format!(
                    "Requested resource not found: Shard does not exist: {}",
                    text(body, "ShardId")
                ),
            ));
        }
        let sequence = || -> Result<u64, ApiError> {
            let given = body
                .get("SequenceNumber")
                .and_then(Json::as_str)
                .ok_or_else(|| {
                    validation("A SequenceNumber is required for this ShardIteratorType")
                })?;
            given
                .parse::<u64>()
                .ok()
                .filter(|n| *n >= 1 && *n <= s.last_seq.max(1))
                .ok_or_else(|| validation(format!("Invalid SequenceNumber {given} for this shard")))
        };
        let after = match text(body, "ShardIteratorType") {
            "TRIM_HORIZON" => 0,
            "LATEST" => s.last_seq,
            "AT_SEQUENCE_NUMBER" => sequence()? - 1,
            "AFTER_SEQUENCE_NUMBER" => sequence()?,
            "AT_TIMESTAMP" => {
                let at = body
                    .get("Timestamp")
                    .and_then(Json::as_f64)
                    .ok_or_else(|| validation("A Timestamp is required for AT_TIMESTAMP"))?;
                // The records before the time: the iterator starts after the last of them.
                #[allow(clippy::cast_possible_truncation)]
                let at_ms = (at * 1000.0) as i64;
                let batch = self
                    .ddb
                    .stream_records(&caller.region, &s.arn, 0, usize::MAX)
                    .await
                    .map_err(|_| stream_not_found(&s.arn))?;
                batch
                    .records
                    .iter()
                    .take_while(|r| r.time_ms < at_ms)
                    .last()
                    .map_or(0, |r| r.seq)
            }
            other => return Err(validation(format!("Invalid ShardIteratorType {other}"))),
        };
        let it = Iterator {
            arn: s.arn,
            after,
            issued_ms: self.ddb.now_ms(),
        };
        Ok(json!({ "ShardIterator": it.write() }))
    }

    async fn records(&self, caller: &Caller, it: Iterator, body: &Json) -> Result<Json, ApiError> {
        let now = self.ddb.now_ms();
        if now - it.issued_ms > ITERATOR_MS {
            return Err(ApiError::modeled(
                "ExpiredIteratorException",
                "Iterator expired. The iterator was created more than 15 minutes ago.",
            ));
        }
        let limit = limit(body, MAX_RECORDS)?;
        let batch = self
            .ddb
            .stream_records(&caller.region, &it.arn, it.after, limit)
            .await
            .map_err(|_| stream_not_found(&it.arn))?;
        let records: Vec<Json> = batch
            .records
            .iter()
            .map(|r| wire_record(r.to_event(&caller.region)))
            .collect();
        let last = batch.records.last().map_or(it.after, |r| r.seq);
        let mut out = json!({ "Records": records });
        // A closed shard that has given its last record has no next iterator.
        if batch.open || !batch.records.is_empty() {
            let next = Iterator {
                arn: it.arn,
                after: last,
                issued_ms: now,
            };
            put(&mut out, "NextShardIterator", json!(next.write()));
        }
        Ok(out)
    }
}

/// A record as `GetRecords` gives it: Lambda's event shape, but the identity's members are capitalised as the model has them.
fn wire_record(mut event: Json) -> Json {
    if let Some(identity) = event.get_mut("userIdentity")
        && let Some(m) = identity.as_object_mut()
    {
        let principal = m.remove("principalId");
        let kind = m.remove("type");
        if let Some(p) = principal {
            m.insert("PrincipalId".into(), p);
        }
        if let Some(t) = kind {
            m.insert("Type".into(), t);
        }
    }
    event
}

/// A position in a stream: after sequence number `after`. Opaque to the caller.
struct Iterator {
    arn: String,
    after: u64,
    issued_ms: i64,
}

impl Iterator {
    fn write(&self) -> String {
        aws_core::base64::encode(
            format!("1|{}|{}|{}", self.after, self.issued_ms, self.arn).as_bytes(),
        )
    }

    fn read(token: &str) -> Result<Self, ApiError> {
        let invalid = || validation("Invalid ShardIterator");
        let raw = aws_core::base64::decode(token).ok_or_else(invalid)?;
        let text = String::from_utf8(raw).map_err(|_| invalid())?;
        let mut parts = text.splitn(4, '|');
        let (Some("1"), Some(after), Some(issued), Some(arn)) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(invalid());
        };
        Ok(Self {
            arn: arn.to_owned(),
            after: after.parse().map_err(|_| invalid())?,
            issued_ms: issued.parse().map_err(|_| invalid())?,
        })
    }
}

/// The text of a sequence number, as the table's stream records give it.
fn sequence_number(n: u64) -> String {
    format!("{n:021}")
}

#[allow(clippy::cast_precision_loss)]
fn seconds(ms: i64) -> f64 {
    ms as f64 / 1000.0
}

/// Sets a member of an object.
fn put(target: &mut Json, member: &str, value: Json) {
    if let Some(m) = target.as_object_mut() {
        m.insert(member.to_owned(), value);
    }
}

fn text<'a>(body: &'a Json, member: &str) -> &'a str {
    body.get(member).and_then(Json::as_str).unwrap_or_default()
}

/// `Limit`, at most `max` (and `max` when absent).
fn limit(body: &Json, max: i64) -> Result<usize, ApiError> {
    match body.get("Limit").and_then(Json::as_i64) {
        None => Ok(usize::try_from(max).unwrap_or(0)),
        Some(n) if n <= max => Ok(usize::try_from(n).unwrap_or(1)),
        Some(n) => Err(validation(format!(
            "1 validation error detected: Value '{n}' at 'limit' failed to satisfy constraint: Member must have value less than or equal to {max}"
        ))),
    }
}

fn stream_arn_any(caller: &Caller) -> String {
    format!(
        "arn:aws:dynamodb:{}:{}:table/*/stream/*",
        caller.region, caller.account_id
    )
}

fn validation(message: impl Into<String>) -> ApiError {
    ApiError::new(400, "ValidationException", message)
}

fn stream_not_found(arn: &str) -> ApiError {
    ApiError::modeled(
        "ResourceNotFoundException",
        format!("Requested resource not found: Stream: {arn} not found"),
    )
}

fn patterns() -> &'static Patterns {
    static PATTERNS: OnceLock<Patterns> = OnceLock::new();
    PATTERNS.get_or_init(|| Patterns::new(&TABLES))
}

impl Service for Streams {
    fn meta(&self) -> &'static aws_schema::meta::ServiceMeta {
        &META
    }

    fn tables(&self) -> &'static aws_schema::tables::Tables {
        &TABLES
    }

    fn call(&self, call: Call) -> BoxFuture<'_, Reply> {
        Box::pin(async move {
            let op = call.op;
            let json = self.ask(call).await?;
            reply_json(&TABLES, patterns(), op, &json)
        })
    }
}

#[cfg(test)]
mod tests;
