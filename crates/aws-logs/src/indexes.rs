//! Field index policies (`PutIndexPolicy`, `DeleteIndexPolicy`, `DescribeIndexPolicies`) and the indexes they make
//! (`DescribeFieldIndexes`). Locally a query reads every event in its range, so an index changes no answer and no cost: the policy
//! is checked as the model describes it, kept, and reported, and the indexes report the times of the first and last event that
//! holds each field. `estimatedRecordsSkipped` in a query's statistics stays 0.

use aws_wire::ApiError;
use serde_json::{Value, json};

use crate::core::Core;
use crate::insights::record::{self, Event as Seen};
use crate::model::{Doc, doc_key, group_arn, stream_key};
use crate::req::{Put, Req, invalid, name_of_identifier, no_group};

pub(crate) const INDEX_POLICY: &str = "index-policy";
/// The model: "As many as 20 fields can be included in the policy. Each field name can include as many as 100 characters."
const MAX_FIELDS: usize = 20;
const MAX_FIELD_LEN: usize = 100;
/// The model's default indexes of a Standard-class group.
const DEFAULTS: [&str; 5] = [
    "@logStream",
    "@aws.region",
    "@aws.account",
    "@source.log",
    "traceId",
];
/// Events read, newest first, to find when each indexed field was first and last seen (a local bound).
const SCAN: usize = 10_000;

/// The fields of a policy document and their types (`FIELD_INDEX` or `FACET`), or why it is refused. Wording (memory).
fn fields_of(doc: &str) -> Result<Vec<(String, String)>, ApiError> {
    let v: Value =
        serde_json::from_str(doc).map_err(|_| invalid("The policy document is not valid JSON."))?;
    let mut out: Vec<(String, String)> = Vec::new();
    if let Some(list) = v.get("Fields") {
        let list = list
            .as_array()
            .ok_or_else(|| invalid("Fields must be an array of field names."))?;
        for f in list {
            let name = f
                .as_str()
                .ok_or_else(|| invalid("Fields must be an array of field names."))?;
            out.push((name.to_owned(), "FIELD_INDEX".into()));
        }
    }
    if let Some(map) = v.get("FieldsV2") {
        let map = map
            .as_object()
            .ok_or_else(|| invalid("FieldsV2 must be an object."))?;
        for (name, spec) in map {
            if out.iter().any(|(n, _)| n == name) {
                return Err(invalid(
                    "Field names within Fields and FieldsV2 must be mutually exclusive.",
                ));
            }
            let kind = spec.get("type").and_then(Value::as_str).unwrap_or_default();
            if kind != "FIELD_INDEX" && kind != "FACET" {
                return Err(invalid(
                    "Each field in FieldsV2 must have a type of FIELD_INDEX or FACET.",
                ));
            }
            out.push((name.clone(), kind.to_owned()));
        }
    }
    if out.is_empty() {
        return Err(invalid(
            "The policy document must include at least one field index.",
        ));
    }
    if out.len() > MAX_FIELDS {
        return Err(ApiError::modeled(
            "LimitExceededException",
            format!("A field index policy can include at most {MAX_FIELDS} fields."),
        ));
    }
    if out
        .iter()
        .any(|(n, _)| n.is_empty() || n.chars().count() > MAX_FIELD_LEN)
    {
        return Err(invalid(format!(
            "Each field name can include at most {MAX_FIELD_LEN} characters."
        )));
    }
    Ok(out)
}

impl Core {
    pub(crate) fn put_index_policy(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = self.identified(r)?;
        if self
            .groups
            .get(&name)
            .is_some_and(|g| g.class != "STANDARD")
        {
            // The model: "Only log groups in the Standard log class support field index policies." Wording (memory).
            return Err(invalid(
                "Field index policies are supported only for log groups in the Standard log class.",
            ));
        }
        fields_of(r.s("policyDocument"))?;
        let policy = json!({
            "logGroupIdentifier": group_arn(r.region(), r.account(), &name),
            "lastUpdateTime": self.now_ms(),
            "policyDocument": r.s("policyDocument"),
            "source": "LOG_GROUP",
        });
        let updated_ms = self.now_ms();
        self.docs.insert(
            doc_key(INDEX_POLICY, &name),
            Doc {
                json: policy.to_string(),
                updated_ms,
            },
        );
        Ok(json!({"indexPolicy": policy}))
    }

    pub(crate) fn delete_index_policy(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = self.identified(r)?;
        self.docs
            .remove(&doc_key(INDEX_POLICY, &name))
            .ok_or_else(|| {
                // (memory)
                ApiError::modeled(
                    "ResourceNotFoundException",
                    "The specified log group does not have a field index policy.",
                )
            })?;
        Ok(json!({}))
    }

    /// The policy that applies to a group: its own, or an account-wide `FIELD_INDEX_POLICY` whose selection takes it.
    fn index_policy(&self, name: &str) -> Option<Value> {
        if let Some(d) = self.docs.get(&doc_key(INDEX_POLICY, name)) {
            return serde_json::from_str(&d.json).ok();
        }
        self.docs
            .with_prefix(&doc_key("account-policy", "FIELD_INDEX_POLICY/"))
            .filter_map(|(_, d)| serde_json::from_str::<Value>(&d.json).ok())
            .find(|p| selects(p.get("selectionCriteria").and_then(Value::as_str), name))
            .map(|p| {
                json!({
                    "policyName": p.get("policyName").cloned().unwrap_or_default(),
                    "policyDocument": p.get("policyDocument").cloned().unwrap_or_default(),
                    "lastUpdateTime": p.get("lastUpdatedTime").cloned().unwrap_or_default(),
                    "source": "ACCOUNT",
                })
            })
    }

    pub(crate) fn describe_index_policies(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let mut out = Vec::new();
        for id in r.strings("logGroupIdentifiers") {
            let name = name_of_identifier(&id);
            self.groups.get(&name).ok_or_else(no_group)?;
            if let Some(mut p) = self.index_policy(&name) {
                p.put(
                    "logGroupIdentifier",
                    json!(group_arn(r.region(), r.account(), &name)),
                );
                out.push(p);
            }
        }
        Ok(json!({"indexPolicies": out}))
    }

    pub(crate) fn describe_field_indexes(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let wanted = r.strings("indexCategories");
        let wants = |c: &str| {
            if wanted.is_empty() {
                c != "AUTO"
            } else {
                wanted.iter().any(|w| w == c)
            }
        };
        let mut out = Vec::new();
        for id in r.strings("logGroupIdentifiers") {
            let name = name_of_identifier(&id);
            self.groups.get(&name).ok_or_else(no_group)?;
            let custom = self
                .index_policy(&name)
                .and_then(|p| {
                    p.get("policyDocument")
                        .and_then(Value::as_str)
                        .and_then(|d| fields_of(d).ok())
                })
                .unwrap_or_default();
            let mut fields: Vec<(String, String, &str)> = Vec::new();
            if wants("DEFAULT") {
                fields.extend(
                    DEFAULTS
                        .iter()
                        .map(|f| ((*f).to_owned(), "FIELD_INDEX".to_owned(), "DEFAULT")),
                );
            }
            if wants("CUSTOM") {
                fields.extend(custom.into_iter().map(|(f, t)| (f, t, "CUSTOM")));
            }
            let seen = self.seen(&name, &fields, r.account());
            let arn = group_arn(r.region(), r.account(), &name);
            for ((field, kind, category), times) in fields.iter().zip(seen) {
                let mut v = json!({
                    "logGroupIdentifier": arn,
                    "fieldIndexName": field,
                    "type": kind,
                    "indexCategory": category,
                    "lastScanTime": self.now_ms(),
                });
                if let Some((first, last)) = times {
                    v.put("firstEventTime", json!(first));
                    v.put("lastEventTime", json!(last));
                }
                out.push(v);
            }
        }
        Ok(json!({"fieldIndexes": out}))
    }

    /// For each field, the times of the first and last of the group's newest events that hold it.
    fn seen(
        &self,
        group: &str,
        fields: &[(String, String, &str)],
        account: &str,
    ) -> Vec<Option<(i64, i64)>> {
        let mut found: Vec<Option<(i64, i64)>> = vec![None; fields.len()];
        let streams: Vec<(u32, String)> = self
            .streams
            .with_prefix(&stream_key(group, ""))
            .map(|(_, s)| (s.no, s.name.clone()))
            .collect();
        for (no, stream) in streams {
            let Ok(rows) = self.rows(no, ("", "g"), SCAN, true, &|_| true) else {
                continue;
            };
            for row in rows {
                let e = Seen {
                    ts: row.ts,
                    ingested: row.event.ingested_ms,
                    message: &row.event.message,
                    transformed: row.event.transformed.as_deref(),
                    stream: &stream,
                    group,
                    account,
                    ptr: String::new(),
                    lambda: false,
                };
                let r = record::row(&e);
                for (slot, (f, _, _)) in found.iter_mut().zip(fields) {
                    if r.get(f).is_some() {
                        *slot = Some(
                            slot.map_or((row.ts, row.ts), |(a, b)| (a.min(row.ts), b.max(row.ts))),
                        );
                    }
                }
            }
        }
        found
    }
}

/// Whether an account policy's selection criteria (`LogGroupNamePrefix IN ["a", "b"]`, or none) take a group. (memory)
fn selects(criteria: Option<&str>, group: &str) -> bool {
    let Some(c) = criteria.filter(|c| !c.trim().is_empty()) else {
        return true;
    };
    let Some((_, list)) = c.split_once('[') else {
        return false;
    };
    let quoted = list.trim_end_matches(|ch: char| ch == ']' || ch.is_whitespace());
    quoted
        .split(',')
        .map(|p| p.trim().trim_matches('"'))
        .filter(|p| !p.is_empty())
        .any(|p| group.starts_with(p))
}
