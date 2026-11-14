//! SCOPE tier B, "store and flag": metric streams, Contributor Insights rules and alarm mute rules are kept, returned by their Get,
//! List and Describe operations, tagged by ARN, and **not active locally**: no metric is streamed to a Firehose, no contributor is
//! counted (`GetInsightRuleReport` answers an empty report), no alarm action is muted. A CDK or Terraform deployment that declares
//! them succeeds and reads back what it wrote; the log says, at every Put, that the thing is stored and not active.
//!
//! The state fields keep AWS's values (`running`/`stopped`, `ENABLED`/`DISABLED`, `SCHEDULED`/`ACTIVE`/`EXPIRED`): a value AWS never
//! sends would stall a deployment tool that waits for one (Terraform waits for a metric stream to be `running`). Managed Contributor
//! Insights rules (`PutManagedInsightRules`, `ListManagedInsightRules`) belong to other services' resources and stay refused by name.

use aws_wire::ApiError;
use serde_json::{Value, json};

use crate::core::Core;
use crate::model::{Doc, doc_key};
use crate::req::{Put, Req, secs};

pub(crate) const STREAM: &str = "metric-stream";
pub(crate) const RULE: &str = "insight-rule";
pub(crate) const MUTE: &str = "alarm-mute-rule";

/// The kind and name an ARN of one of these names, for tagging.
pub(crate) fn key_of_arn(arn: &str) -> Option<String> {
    let (kind, name) = [
        (":metric-stream/", STREAM),
        (":insight-rule/", RULE),
        (":alarm-mute-rule:", MUTE),
    ]
    .into_iter()
    .find_map(|(sep, kind)| arn.split_once(sep).map(|(_, name)| (kind, name)))?;
    Some(doc_key(kind, name))
}

pub(crate) fn arn(r: &Req<'_>, kind: &str, name: &str) -> String {
    let sep = if kind == MUTE { ':' } else { '/' };
    format!(
        "arn:aws:cloudwatch:{}:{}:{kind}{sep}{name}",
        r.region(),
        r.account()
    )
}

pub(crate) fn not_found(what: &str, name: &str) -> ApiError {
    // (memory)
    ApiError::modeled(
        "ResourceNotFoundException",
        format!("The {what} {name} does not exist."),
    )
}

fn flag(kind: &str, name: &str) {
    tracing::info!(
        "cloudwatch: {kind} {name} is stored, not active locally (doze-aws keeps it and returns it; nothing runs it)"
    );
}

impl Core {
    /// Keeps `body` (the resource in its output shape, timestamps in milliseconds) under `kind`/`name`; tags only when it is new,
    /// as `Tags` on a Put of an existing resource is ignored by AWS (memory).
    pub(crate) fn keep(&mut self, kind: &str, name: &str, mut body: Value, r: &Req<'_>) {
        let key = doc_key(kind, name);
        let now = self.now_ms();
        let (tags, created) = match self.docs.get(&key) {
            Some(d) => (d.tags.clone(), stored(d).get("_created").cloned()),
            None => (r.tags().into_iter().collect(), None),
        };
        body.put("_created", created.unwrap_or_else(|| json!(now)));
        self.docs.insert(
            key,
            Doc {
                json: body.to_string(),
                updated_ms: now,
                tags,
            },
        );
        flag(kind, name);
    }

    pub(crate) fn all(&self, kind: &str) -> Vec<(String, Value, i64)> {
        let base = doc_key(kind, "");
        self.docs
            .with_prefix(&base)
            .filter_map(|(k, d)| Some((k.strip_prefix(&base)?.to_owned(), stored(d), d.updated_ms)))
            .collect()
    }

    pub(crate) fn one(&self, kind: &str, name: &str) -> Option<(Value, i64)> {
        self.docs
            .get(&doc_key(kind, name))
            .map(|d| (stored(d), d.updated_ms))
    }

    pub(crate) fn put_metric_stream(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = r.s("Name").to_owned();
        if r.v
            .get("IncludeFilters")
            .is_some_and(|v| v.as_array().is_some_and(|a| !a.is_empty()))
            && r.v
                .get("ExcludeFilters")
                .is_some_and(|v| v.as_array().is_some_and(|a| !a.is_empty()))
        {
            // (memory)
            return Err(ApiError::modeled(
                "InvalidParameterCombinationException",
                "You cannot specify both IncludeFilters and ExcludeFilters.",
            ));
        }
        let state = self
            .one(STREAM, &name)
            .and_then(|(v, _)| v.get("State").cloned());
        let mut body = json!({"State": state.unwrap_or_else(|| json!("running"))});
        for k in [
            "IncludeFilters",
            "ExcludeFilters",
            "FirehoseArn",
            "RoleArn",
            "OutputFormat",
            "StatisticsConfigurations",
            "IncludeLinkedAccountsMetrics",
        ] {
            if let Some(v) = r.v.get(k) {
                body.put(k, v.clone());
            }
        }
        self.keep(STREAM, &name, body, r);
        Ok(json!({"Arn": arn(r, STREAM, &name)}))
    }

    fn stream_json(r: &Req<'_>, name: &str, v: &Value, updated_ms: i64) -> Value {
        let mut out = v.clone();
        strip(&mut out);
        out.put("Name", json!(name));
        out.put("Arn", json!(arn(r, STREAM, name)));
        let created = v
            .get("_created")
            .and_then(Value::as_i64)
            .unwrap_or(updated_ms);
        out.put("CreationDate", secs(created));
        out.put("LastUpdateDate", secs(updated_ms));
        out
    }

    pub(crate) fn get_metric_stream(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = r.s("Name");
        let (v, at) = self
            .one(STREAM, name)
            .ok_or_else(|| not_found("metric stream", name))?;
        Ok(Self::stream_json(r, name, &v, at))
    }

    pub(crate) fn list_metric_streams(&self, r: &Req<'_>) -> Value {
        let entries: Vec<Value> = self
            .all(STREAM)
            .iter()
            .map(|(name, v, at)| {
                let full = Self::stream_json(r, name, v, *at);
                let keep = [
                    "Arn",
                    "CreationDate",
                    "LastUpdateDate",
                    "Name",
                    "FirehoseArn",
                    "State",
                    "OutputFormat",
                ];
                Value::Object(
                    keep.iter()
                        .filter_map(|k| Some(((*k).to_owned(), full.get(*k)?.clone())))
                        .collect(),
                )
            })
            .collect();
        page(entries, r, "MaxResults", "Entries")
    }

    pub(crate) fn delete_stored(&mut self, kind: &str, name: &str) -> Value {
        self.docs.remove(&doc_key(kind, name));
        json!({})
    }

    /// `StartMetricStreams`/`StopMetricStreams`, and `EnableInsightRules`/`DisableInsightRules`: sets the state of each named one.
    /// Insight rules answer the names not found as failures; metric streams ignore them (memory).
    pub(crate) fn set_states(&mut self, kind: &str, names: &[String], state: &str) -> Value {
        let mut missing = Vec::new();
        for name in names {
            let key = doc_key(kind, name);
            let Some(d) = self.docs.get_mut(&key) else {
                missing.push(name.clone());
                continue;
            };
            let mut v = stored(d);
            v.put("State", json!(state));
            d.json = v.to_string();
        }
        if kind == STREAM {
            json!({})
        } else {
            json!({"Failures": failures(&missing)})
        }
    }
}

/// The partial failures for rules that do not exist. (memory)
pub(crate) fn failures(missing: &[String]) -> Vec<Value> {
    missing
        .iter()
        .map(|name| {
            json!({"FailureResource": name, "ExceptionType": "ResourceNotFoundException",
                "FailureCode": "404", "FailureDescription": format!("The rule {name} does not exist.")})
        })
        .collect()
}

/// The stored body of a document (an empty object if it does not read).
pub(crate) fn stored(d: &Doc) -> Value {
    serde_json::from_str(&d.json).unwrap_or_else(|_| json!({}))
}

/// Takes off the keys kept for doze-aws itself (they start with `_`).
pub(crate) fn strip(v: &mut Value) {
    if let Some(o) = v.as_object_mut() {
        o.retain(|k, _| !k.starts_with('_'));
    }
}

/// A page of `items` by a numeric `NextToken` (an offset), with the page size from `limit_key` (default 500, as AWS's lists here).
pub(crate) fn page(items: Vec<Value>, r: &Req<'_>, limit_key: &str, list_key: &str) -> Value {
    let skip = r
        .opt("NextToken")
        .and_then(|t| t.parse::<usize>().ok())
        .unwrap_or(0);
    let limit = r
        .int(limit_key)
        .and_then(|n| usize::try_from(n).ok())
        .filter(|n| *n > 0)
        .unwrap_or(500);
    let total = items.len();
    let mut out = json!({ list_key: items.into_iter().skip(skip).take(limit).collect::<Vec<_>>() });
    if skip + limit < total {
        out.put("NextToken", json!((skip + limit).to_string()));
    }
    out
}
