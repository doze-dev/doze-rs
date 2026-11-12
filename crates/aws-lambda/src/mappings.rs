//! Event source mappings: the configuration (SQS queues, Kinesis streams, DynamoDB streams), checked when it is made. The pollers that
//! act on it are in `poll/`.

use aws_wire::ApiError;
use serde_json::{Map, Value, json};

use crate::arn;
use crate::core::{Core, Out, Staged};
use crate::errors::{invalid, not_found};
use crate::functions::{page, uuid};
use crate::model::Mapping;
use crate::req::Req;

/// The members a caller sets on a mapping (all of them kept; the ones a poller reads are in `poll/`).
const SETTABLE: &[&str] = &[
    "BatchSize",
    "MaximumBatchingWindowInSeconds",
    "ParallelizationFactor",
    "StartingPosition",
    "StartingPositionTimestamp",
    "DestinationConfig",
    "MaximumRecordAgeInSeconds",
    "BisectBatchOnFunctionError",
    "MaximumRetryAttempts",
    "TumblingWindowInSeconds",
    "FunctionResponseTypes",
    "ScalingConfig",
    "FilterCriteria",
    "SourceAccessConfigurations",
    "KMSKeyArn",
    "MetricsConfig",
    "LoggingConfig",
    "ProvisionedPollerConfig",
];

/// What kind of source an ARN names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Source {
    Sqs,
    Kinesis,
    DynamoDb,
}

impl Source {
    pub(crate) fn of(arn: &str) -> Option<Self> {
        let service = arn.split(':').nth(2)?;
        match service {
            "sqs" => Some(Self::Sqs),
            "kinesis" if arn.contains(":stream/") => Some(Self::Kinesis),
            "dynamodb" if arn.contains("/stream/") => Some(Self::DynamoDb),
            _ => None,
        }
    }

    /// The default batch size.
    fn batch(self) -> i64 {
        if self == Self::Sqs { 10 } else { 100 }
    }
}

fn state_name(enabled: bool) -> &'static str {
    if enabled { "Enabled" } else { "Disabled" }
}

impl Core {
    /// The mapping as the API shows it. `state` overrides the settled one (`Creating` in the answer to a create).
    fn mapping_json(&self, m: &Mapping, state: Option<&str>) -> Value {
        let mut out: Map<String, Value> = serde_json::from_str(&m.config).unwrap_or_default();
        out.insert("UUID".into(), json!(m.uuid));
        out.insert(
            "EventSourceMappingArn".into(),
            json!(arn::mapping(&self.region, account_of(&out), &m.uuid)),
        );
        out.insert(
            "State".into(),
            json!(state.unwrap_or_else(|| state_name(m.enabled))),
        );
        out.insert("StateTransitionReason".into(), json!("USER_INITIATED"));
        out.insert("LastModified".into(), json!(m.modified_ms as f64 / 1000.0));
        if let Some(result) = &m.last_result {
            out.insert("LastProcessingResult".into(), json!(result));
        }
        Value::Object(out)
    }

    pub(crate) fn create_mapping(&mut self, r: &Req, _staged: &Staged) -> Result<Out, ApiError> {
        check_filters(r)?;
        let source_arn = r.text("EventSourceArn");
        let Some(source) = Source::of(source_arn) else {
            return Err(unsupported_source(source_arn));
        };
        let f = r.function()?;
        let function = self.find(r, &f)?;
        let target = self.arn(r, &f.name, f.qualifier.as_deref());
        let mut cfg: Map<String, Value> = Map::new();
        for k in SETTABLE {
            if let Some(v) = r.value(k) {
                cfg.insert((*k).to_owned(), v.clone());
            }
        }
        cfg.insert("FunctionArn".into(), json!(target));
        cfg.insert("EventSourceArn".into(), json!(source_arn));
        cfg.insert("AccountId".into(), json!(r.account()));
        cfg.entry("BatchSize").or_insert(json!(source.batch()));
        cfg.entry("MaximumBatchingWindowInSeconds")
            .or_insert(json!(0));
        cfg.entry("FunctionResponseTypes").or_insert(json!([]));
        if source != Source::Sqs {
            cfg.entry("StartingPosition").or_insert(json!("LATEST"));
            cfg.entry("ParallelizationFactor").or_insert(json!(1));
            cfg.entry("MaximumRecordAgeInSeconds").or_insert(json!(-1));
            cfg.entry("MaximumRetryAttempts").or_insert(json!(-1));
            cfg.entry("BisectBatchOnFunctionError")
                .or_insert(json!(false));
            cfg.entry("TumblingWindowInSeconds").or_insert(json!(0));
        }
        let _ = function;
        let enabled = r.flag("Enabled").unwrap_or(true);
        let id = uuid()?;
        let m = Mapping {
            uuid: id.clone(),
            config: Value::Object(cfg).to_string(),
            enabled,
            modified_ms: self.now_ms(),
            last_result: None,
            tags: crate::functions::tags_of(r.value("Tags")),
        };
        let shown = self.mapping_json(&m, Some(if enabled { "Creating" } else { "Disabled" }));
        self.mappings.insert(id, m);
        Ok(Out {
            json: shown,
            status: Some(202),
        })
    }

    fn mapping_of(&self, r: &Req) -> Result<&Mapping, ApiError> {
        self.mappings
            .get(r.text("UUID"))
            .ok_or_else(|| not_found("The resource you requested does not exist."))
    }

    pub(crate) fn get_mapping(&self, r: &Req) -> Result<Out, ApiError> {
        Ok(self.mapping_json(self.mapping_of(r)?, None).into())
    }

    pub(crate) fn update_mapping(&mut self, r: &Req) -> Result<Out, ApiError> {
        check_filters(r)?;
        let mut m = self.mapping_of(r)?.clone();
        let mut cfg: Map<String, Value> = serde_json::from_str(&m.config).unwrap_or_default();
        for k in SETTABLE {
            if let Some(v) = r.value(k) {
                cfg.insert((*k).to_owned(), v.clone());
            }
        }
        if r.opt("FunctionName").is_some() {
            let f = r.function()?;
            self.find(r, &f)?;
            cfg.insert(
                "FunctionArn".into(),
                json!(self.arn(r, &f.name, f.qualifier.as_deref())),
            );
        }
        if let Some(e) = r.flag("Enabled") {
            m.enabled = e;
        }
        m.config = Value::Object(cfg).to_string();
        m.modified_ms = self.now_ms();
        let shown = self.mapping_json(&m, Some("Updating"));
        self.mappings.insert(m.uuid.clone(), m);
        Ok(Out {
            json: shown,
            status: Some(202),
        })
    }

    pub(crate) fn delete_mapping(&mut self, r: &Req) -> Result<Out, ApiError> {
        let m = self.mapping_of(r)?.clone();
        self.mappings.remove(&m.uuid);
        let prefix = format!("{}/", m.uuid);
        let keys: Vec<String> = self
            .checkpoints
            .with_prefix(&prefix)
            .map(|(k, _)| k.to_owned())
            .collect();
        for k in keys {
            self.checkpoints.remove(&k);
        }
        Ok(Out {
            json: self.mapping_json(&m, Some("Deleting")),
            status: Some(202),
        })
    }

    pub(crate) fn list_mappings(&self, r: &Req) -> Result<Out, ApiError> {
        let (source, function) = (
            r.opt("EventSourceArn"),
            r.function()
                .ok()
                .filter(|_| r.opt("FunctionName").is_some()),
        );
        let mut rows: Vec<Value> = self
            .mappings
            .iter()
            .map(|(_, m)| self.mapping_json(m, None))
            .filter(|v| {
                source.is_none_or(|s| v.get("EventSourceArn").and_then(Value::as_str) == Some(s))
            })
            .filter(|v| {
                function.as_ref().is_none_or(|f| {
                    v.get("FunctionArn")
                        .and_then(Value::as_str)
                        .is_some_and(|a| a.split(':').nth(6) == Some(f.name.as_str()))
                })
            })
            .collect();
        rows.sort_by_key(|v| v.get("UUID").and_then(Value::as_str).map(str::to_owned));
        Ok(page(r, rows, "EventSourceMappings", |v| {
            v.get("UUID")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        })
        .into())
    }

    /// The mapping as a poller reads it (the API object, its enabled flag and its generation).
    pub(crate) fn mapping_view(&self, uuid: &str) -> Option<Value> {
        let m = self.mappings.get(uuid)?;
        let mut v = self.mapping_json(m, None);
        crate::set(&mut v, "Enabled", json!(m.enabled));
        Some(v)
    }

    /// Every enabled mapping, for starting pollers.
    pub(crate) fn enabled_mappings(&self) -> Vec<Value> {
        self.mappings
            .iter()
            .filter(|(_, m)| m.enabled)
            .filter_map(|(u, _)| self.mapping_view(u))
            .collect()
    }

    /// The checks of a mapping's source that need the source service: the queue exists and hides messages at least as long as the
    /// function runs (SQS); the stream exists.
    pub(crate) async fn check_source(&self, r: &Req, update: bool) -> Result<(), ApiError> {
        let Some(peers) = self.peers.clone() else {
            return Ok(());
        };
        let (source_arn, function_name) = if update {
            let m = self.mapping_of(r)?;
            let cfg: Value = serde_json::from_str(&m.config).unwrap_or(Value::Null);
            let text = |k: &str| {
                cfg.get(k)
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned()
            };
            (
                text("EventSourceArn"),
                r.opt("FunctionName")
                    .map_or_else(|| text("FunctionArn"), str::to_owned),
            )
        } else {
            (
                r.text("EventSourceArn").to_owned(),
                r.text("FunctionName").to_owned(),
            )
        };
        let Some(source) = Source::of(&source_arn) else {
            return Err(unsupported_source(&source_arn));
        };
        let f = arn::parse(&function_name, None, &self.region, r.account())?;
        let function = self.find(r, &f)?;
        let timeout = serde_json::from_str::<Value>(&function.latest.config)
            .ok()
            .and_then(|c| c.get("Timeout").and_then(Value::as_i64))
            .unwrap_or(3);
        let acting = &r.caller;
        match source {
            Source::Sqs => crate::poll::sqs::check(&peers, acting, &source_arn, timeout).await,
            Source::Kinesis => {
                let asked = json!({"StreamARN": source_arn});
                peers.call("lambda", acting, "kinesis", "DescribeStreamSummary", &asked).await.map(|_| ()).map_err(|e| {
                    invalid(format!("Cannot access stream {source_arn}. Please ensure the role can perform the GetRecords, GetShardIterator, DescribeStream, and ListShards Kinesis actions on your stream in IAM. ({})", e.message))
                })
            }
            Source::DynamoDb => Ok(()),
        }
    }
}

/// The filter patterns of a mapping must be valid patterns.
fn check_filters(r: &Req) -> Result<(), ApiError> {
    let filters = r
        .value("FilterCriteria")
        .and_then(|c| c.get("Filters"))
        .and_then(Value::as_array);
    for f in filters.into_iter().flatten() {
        if !f
            .get("Pattern")
            .and_then(Value::as_str)
            .is_some_and(crate::poll::filter::valid)
        {
            return Err(invalid("Invalid filter pattern definition."));
        }
    }
    Ok(())
}

fn account_of(cfg: &Map<String, Value>) -> &str {
    cfg.get("AccountId")
        .and_then(Value::as_str)
        .unwrap_or("000000000000")
}

/// A source Lambda has and doze-aws does not poll (Kafka, MQ, DocumentDB, self-managed): refused by name, never faked.
fn unsupported_source(arn: &str) -> ApiError {
    if Source::of(arn).is_none() && arn.starts_with("arn:") {
        ApiError::new(
            501,
            "NotImplemented",
            format!(
                "Event source {arn} is not built in doze-aws: only SQS queues, Kinesis streams and DynamoDB streams are polled."
            ),
        )
    } else {
        invalid(format!("Invalid event source ARN: {arn}"))
    }
}
