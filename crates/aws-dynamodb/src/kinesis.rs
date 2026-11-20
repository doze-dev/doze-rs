//! Kinesis Data Streams for DynamoDB: `EnableKinesisStreamingDestination`, `DisableKinesisStreamingDestination`,
//! `DescribeKinesisStreamingDestination`, `UpdateKinesisStreamingDestination`. While a destination is `ACTIVE`, every change of an
//! item is put on the Kinesis stream (over the bus, after the call is answered: `work.rs`) as a record in the shape the developer
//! guide's consumer reads: `eventName`, `dynamodb` with `ApproximateCreationDateTime` in the precision asked for (the precision itself only when asked for), `Keys`,
//! `NewImage` and `OldImage` (both, whatever the table's own stream view), `SizeBytes`.
//!
//! Enabling answers `ENABLING`; the stream is looked up outside the actor and the destination is `ACTIVE` (or `ENABLE_FAILED`, the
//! reason in `DestinationStatusDescription`) by the time the call answers. Disabling answers `DISABLING` and is `DISABLED` at once.
//! The record's members beyond the guide's sample, the partition key (the item's partition key value) and the messages are `(memory)`.

use aws_expr::{Item, item_size, item_to_json};
use aws_wire::ApiError;
use serde_json::{Value as Json, json};

use crate::core::{Core, Cx};
use crate::extras::{Destination, Extra, kinesis_key};
use crate::meta::Table;
use crate::req::{str_of, table_not_found, validation};
use crate::work::Work;

const MILLISECOND: &str = "MILLISECOND";

fn in_use(message: impl Into<String>) -> ApiError {
    ApiError::modeled("ResourceInUseException", message)
}

impl Core {
    fn destinations(&self, table: &str) -> Vec<Destination> {
        match self.extras.get(&kinesis_key(table)) {
            Some(Extra::Kinesis(d)) => d.clone(),
            _ => Vec::new(),
        }
    }

    fn set_destinations(&mut self, table: &str, list: Vec<Destination>) {
        if list.is_empty() {
            self.extras.remove(&kinesis_key(table));
        } else {
            self.extras.insert(kinesis_key(table), Extra::Kinesis(list));
        }
    }

    /// The table and stream a destination request names, both checked.
    fn kinesis_target(&self, cx: &Cx<'_>) -> Result<(String, String), ApiError> {
        let name = cx.table_name("TableName")?;
        if self.tables.get(&name).is_none() {
            return Err(table_not_found(&name));
        }
        let arn = str_of(&cx.body, "StreamArn").unwrap_or_default().to_owned();
        let parts: Vec<&str> = arn.splitn(6, ':').collect();
        let ok = parts.len() == 6
            && parts.get(2) == Some(&"kinesis")
            && parts.get(5).is_some_and(|r| r.starts_with("stream/"));
        if !ok {
            return Err(validation(format!("Invalid StreamArn: {arn}")));
        }
        Ok((name, arn))
    }

    pub(crate) fn enable_kinesis(&mut self, cx: &Cx<'_>) -> Result<Json, ApiError> {
        let (name, arn) = self.kinesis_target(cx)?;
        // Recorded 2026-11-19: the precision is shown (on the answer, the destination and the records) only when the caller set it.
        let precision = cx
            .body
            .get("EnableKinesisStreamingConfiguration")
            .and_then(|c| str_of(c, "ApproximateCreationDateTimePrecision"))
            .map(str::to_owned);
        let mut list = self.destinations(&name);
        let live =
            |d: &Destination| matches!(d.status.as_str(), "ACTIVE" | "ENABLING" | "UPDATING");
        if list.iter().any(|d| d.stream_arn == arn && live(d)) {
            // Recorded 2026-11-19.
            return Err(validation(
                "Table is not in a valid state to enable Kinesis Streaming Destination: EnableKinesisStreamingDestination must be DISABLED or ENABLE_FAILED to perform ENABLE operation.",
            ));
        }
        if list.iter().any(live) {
            return Err(in_use(format!(
                "Table {name} already has an enabled Kinesis streaming destination; disable it first"
            )));
        }
        list.retain(|d| d.stream_arn != arn);
        list.push(Destination {
            stream_arn: arn.clone(),
            status: "ENABLING".into(),
            description: None,
            precision: precision.clone(),
        });
        self.set_destinations(&name, list);
        self.work.push(Work::CheckStream {
            caller: Box::new(cx.call.caller.clone()),
            table: name.clone(),
            stream_arn: arn.clone(),
        });
        let mut config = json!({});
        if let Some(p) = precision {
            crate::req::set(
                &mut config,
                "ApproximateCreationDateTimePrecision",
                json!(p),
            );
        }
        Ok(json!({
            "TableName": name,
            "StreamArn": arn,
            "DestinationStatus": "ENABLING",
            "EnableKinesisStreamingConfiguration": config,
        }))
    }

    /// The stream a destination names was looked up.
    pub(crate) fn stream_checked(&mut self, table: &str, arn: &str, found: Result<(), String>) {
        let mut list = self.destinations(table);
        if let Some(d) = list
            .iter_mut()
            .find(|d| d.stream_arn == arn && d.status == "ENABLING")
        {
            match found {
                Ok(()) => {
                    d.status = "ACTIVE".into();
                    d.description = None;
                }
                Err(why) => {
                    d.status = "ENABLE_FAILED".into();
                    d.description =
                        Some(format!("The Kinesis data stream could not be used: {why}"));
                }
            }
        }
        self.set_destinations(table, list);
    }

    pub(crate) fn disable_kinesis(&mut self, cx: &Cx<'_>) -> Result<Json, ApiError> {
        let (name, arn) = self.kinesis_target(cx)?;
        let mut list = self.destinations(&name);
        let Some(d) = list.iter_mut().find(|d| {
            d.stream_arn == arn && matches!(d.status.as_str(), "ACTIVE" | "ENABLING" | "UPDATING")
        }) else {
            return Err(in_use(format!(
                "Table {name} is not streaming to the Kinesis data stream {arn}"
            )));
        };
        d.status = "DISABLED".into();
        self.set_destinations(&name, list);
        let mut out = json!({
            "TableName": name,
            "StreamArn": arn,
            "DestinationStatus": "DISABLING",
        });
        // Recorded 2026-11-19: the configuration is echoed only when the request carried one.
        if let Some(c) = cx.body.get("EnableKinesisStreamingConfiguration") {
            crate::req::set(&mut out, "EnableKinesisStreamingConfiguration", c.clone());
        }
        Ok(out)
    }

    pub(crate) fn describe_kinesis(&mut self, cx: &Cx<'_>) -> Result<Json, ApiError> {
        let name = cx.table_name("TableName")?;
        if self.tables.get(&name).is_none() {
            return Err(table_not_found(&name));
        }
        let shown: Vec<Json> = self
            .destinations(&name)
            .iter()
            .map(|d| {
                let mut out = json!({
                    "StreamArn": d.stream_arn,
                    "DestinationStatus": d.status,
                });
                if let Some(p) = &d.precision {
                    crate::req::set(&mut out, "ApproximateCreationDateTimePrecision", json!(p));
                }
                if let Some(why) = &d.description {
                    crate::req::set(&mut out, "DestinationStatusDescription", json!(why));
                }
                out
            })
            .collect();
        Ok(json!({"TableName": name, "KinesisDataStreamDestinations": shown}))
    }

    pub(crate) fn update_kinesis(&mut self, cx: &Cx<'_>) -> Result<Json, ApiError> {
        let (name, arn) = self.kinesis_target(cx)?;
        let Some(precision) = cx
            .body
            .get("UpdateKinesisStreamingConfiguration")
            .and_then(|c| str_of(c, "ApproximateCreationDateTimePrecision"))
            .map(str::to_owned)
        else {
            return Err(validation(
                "UpdateKinesisStreamingConfiguration must contain ApproximateCreationDateTimePrecision",
            ));
        };
        let mut list = self.destinations(&name);
        let Some(d) = list
            .iter_mut()
            .find(|d| d.stream_arn == arn && d.status == "ACTIVE")
        else {
            return Err(in_use(format!(
                "Table {name} has no active Kinesis streaming destination {arn}"
            )));
        };
        if d.precision.as_deref().unwrap_or(MILLISECOND) == precision {
            return Err(validation(format!(
                "The Kinesis streaming destination already uses ApproximateCreationDateTimePrecision {precision}"
            )));
        }
        d.precision = Some(precision.clone());
        self.set_destinations(&name, list);
        Ok(json!({
            "TableName": name,
            "StreamArn": arn,
            "DestinationStatus": "UPDATING",
            "UpdateKinesisStreamingConfiguration": {"ApproximateCreationDateTimePrecision": precision},
        }))
    }

    /// Queues the record of a change for each active destination of the table.
    pub(crate) fn queue_kinesis(
        &mut self,
        t: &Table,
        now_ms: i64,
        old: Option<&Item>,
        new: Option<&Item>,
        by_ttl: bool,
    ) {
        let Some(Extra::Kinesis(list)) = self.extras.get(&kinesis_key(&t.name)) else {
            return;
        };
        let active: Vec<(String, Option<String>)> = list
            .iter()
            .filter(|d| d.status == "ACTIVE")
            .map(|d| (d.stream_arn.clone(), d.precision.clone()))
            .collect();
        for (stream_arn, precision) in active {
            let change = Change {
                now_ms,
                old,
                new,
                by_ttl,
            };
            let (key, record) = record(t, &stream_arn, precision.as_deref(), &change);
            let data = record.to_string().into_bytes();
            match self.work.iter_mut().find_map(|w| match w {
                Work::Deliver {
                    stream_arn: s,
                    records,
                } if *s == stream_arn => Some(records),
                _ => None,
            }) {
                Some(records) => records.push((key, data)),
                None => self.work.push(Work::Deliver {
                    stream_arn,
                    records: vec![(key, data)],
                }),
            }
        }
    }
}

/// The record of one change, and its partition key.
/// One change of an item.
struct Change<'a> {
    now_ms: i64,
    old: Option<&'a Item>,
    new: Option<&'a Item>,
    by_ttl: bool,
}

fn record(t: &Table, stream_arn: &str, precision: Option<&str>, c: &Change<'_>) -> (String, Json) {
    let Change {
        now_ms,
        old,
        new,
        by_ttl,
    } = *c;
    let basis = new.or(old).cloned().unwrap_or_default();
    let keys = crate::stage::key_of(t, &basis);
    let partition = keys
        .get(&t.hash.name)
        .map_or_else(|| t.name.clone(), |v| v.to_json().to_string());
    let event = match (old, new) {
        (None, _) => "INSERT",
        (Some(_), Some(_)) => "MODIFY",
        (Some(_), None) => "REMOVE",
    };
    let time = if precision == Some("MICROSECOND") {
        now_ms.saturating_mul(1000)
    } else {
        now_ms
    };
    let size = item_size(&keys) + new.map_or(0, item_size) + old.map_or(0, item_size);
    let mut inner = json!({
        "ApproximateCreationDateTime": time,
        "Keys": item_to_json(&keys),
        "SizeBytes": size,
    });
    if let Some(p) = precision {
        crate::req::set(&mut inner, "ApproximateCreationDateTimePrecision", json!(p));
    }
    if let Some(n) = new {
        crate::req::set(&mut inner, "NewImage", item_to_json(n));
    }
    if let Some(o) = old {
        crate::req::set(&mut inner, "OldImage", item_to_json(o));
    }
    let region = stream_arn.split(':').nth(3).unwrap_or_default();
    let identity = if by_ttl {
        json!({"type": "Service", "principalId": "dynamodb.amazonaws.com"})
    } else {
        Json::Null
    };
    let record = json!({
        "awsRegion": region,
        "eventID": aws_seal::uuid_v4().unwrap_or_default(),
        "eventName": event,
        "userIdentity": identity,
        "recordFormat": "application/json",
        "tableName": t.name,
        "dynamodb": inner,
        "eventSource": "aws:dynamodb",
    });
    (partition, record)
}
