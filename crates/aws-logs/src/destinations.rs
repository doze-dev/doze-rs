//! Destinations: a named Kinesis stream (or Firehose stream) that subscription filters of other accounts deliver to, behind an
//! access policy. One account runs locally, so the access policy is kept and shown and not evaluated; a subscription filter whose
//! `destinationArn` is a destination of this region delivers to the destination's Kinesis stream through the delivery that
//! subscription filters already make (`forward`). A Firehose target is kept and not delivered (doze-aws does not run Firehose).

use aws_wire::ApiError;
use serde_json::{Value, json};

use crate::core::Core;
use crate::model::{Doc, doc_key};
use crate::req::{Put, Req, invalid};

pub(crate) const DESTINATION: &str = "destination";

/// (memory)
fn no_destination() -> ApiError {
    ApiError::modeled(
        "ResourceNotFoundException",
        "The specified destination does not exist.",
    )
}

impl Core {
    fn destination(&self, name: &str) -> Option<Value> {
        self.docs
            .get(&doc_key(DESTINATION, name))
            .and_then(|d| serde_json::from_str(&d.json).ok())
    }

    fn keep_destination(&mut self, name: &str, v: &Value) {
        let updated_ms = self.now_ms();
        self.docs.insert(
            doc_key(DESTINATION, name),
            Doc {
                json: v.to_string(),
                updated_ms,
            },
        );
    }

    pub(crate) fn put_destination(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = r.s("destinationName").to_owned();
        let target = r.s("targetArn");
        let stream = target.starts_with("arn:aws:kinesis:") && target.contains(":stream/");
        let firehose =
            target.starts_with("arn:aws:firehose:") && target.contains(":deliverystream/");
        if !stream && !firehose {
            // (memory)
            return Err(invalid(
                "The targetArn must be the ARN of a Kinesis data stream or an Amazon Data Firehose stream.",
            ));
        }
        let old = self.destination(&name);
        let created = old
            .as_ref()
            .and_then(|o| o.get("creationTime").and_then(Value::as_i64))
            .unwrap_or_else(|| self.now_ms());
        let mut d = json!({
            "destinationName": name,
            "targetArn": target,
            "roleArn": r.s("roleArn"),
            "arn": format!("arn:aws:logs:{}:{}:destination:{name}", r.region(), r.account()),
            "creationTime": created,
        });
        for kept in ["accessPolicy", "tags"] {
            if let Some(v) = old.as_ref().and_then(|o| o.get(kept)) {
                d.put(kept, v.clone());
            }
        }
        if let Some(tags) = r.v.get("tags").filter(|t| t.is_object()) {
            d.put("tags", tags.clone());
        }
        self.keep_destination(&name, &d);
        Ok(json!({"destination": shown(&d)}))
    }

    pub(crate) fn put_destination_policy(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = r.s("destinationName");
        // The model gives this operation no ResourceNotFoundException. (memory)
        let mut d = self
            .destination(name)
            .ok_or_else(|| invalid("The specified destination does not exist."))?;
        let policy = r.s("accessPolicy");
        if policy.len() > 5120
            || serde_json::from_str::<Value>(policy).map_or(true, |p| !p.is_object())
        {
            // The model: "This can be up to 5120 bytes." The wording is (memory).
            return Err(invalid(
                "The access policy is not a valid policy document of at most 5120 bytes.",
            ));
        }
        d.put("accessPolicy", json!(policy));
        self.keep_destination(name, &d);
        Ok(json!({}))
    }

    pub(crate) fn delete_destination(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        self.docs
            .remove(&doc_key(DESTINATION, r.s("destinationName")))
            .ok_or_else(no_destination)?;
        Ok(json!({}))
    }

    pub(crate) fn describe_destinations(&self, r: &Req<'_>) -> Value {
        let prefix = r.opt("DestinationNamePrefix").unwrap_or_default();
        let all: Vec<(String, Value)> = self
            .docs
            .with_prefix(&doc_key(DESTINATION, prefix))
            .filter_map(|(_, d)| serde_json::from_str::<Value>(&d.json).ok())
            .map(|v| {
                let name = v
                    .get("destinationName")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
                (name, v)
            })
            .collect();
        let refs: Vec<&(String, Value)> = all.iter().collect();
        let (page, token) = crate::groups::paged(&refs, r, 50, |(n, _)| n.as_str());
        let mut out =
            json!({"destinations": page.iter().map(|(_, v)| shown(v)).collect::<Vec<_>>()});
        if let Some(t) = token {
            out.put("nextToken", json!(t));
        }
        out
    }

    /// Where a subscription filter's lines go: a destination of this region's target, or the ARN itself.
    pub(crate) fn target_of(&self, arn: &str) -> String {
        let Some((_, name)) = arn.split_once(":destination:") else {
            return arn.to_owned();
        };
        self.destination(name)
            .and_then(|d| {
                d.get("targetArn")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| arn.to_owned())
    }

    /// The tags of a destination after `change`, which are kept only if they are within the tag limit (`None` when there is no such
    /// destination).
    pub(crate) fn destination_tags(
        &mut self,
        arn: &str,
        change: impl FnOnce(&mut serde_json::Map<String, Value>),
    ) -> Option<Value> {
        let (_, name) = arn.split_once(":destination:")?;
        let mut d = self.destination(name)?;
        let tags = changed_tags(&mut d, change);
        if tags
            .as_object()
            .is_some_and(|m| m.len() <= crate::groups::MAX_TAGS)
        {
            self.keep_destination(name, &d);
        }
        Some(tags)
    }
}

/// The `tags` member of a stored document after `change`, set back in it.
pub(crate) fn changed_tags(
    doc: &mut Value,
    change: impl FnOnce(&mut serde_json::Map<String, Value>),
) -> Value {
    let mut tags = doc
        .get("tags")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    change(&mut tags);
    doc.put("tags", Value::Object(tags.clone()));
    Value::Object(tags)
}

/// A destination as the API shows it: without its tags.
fn shown(d: &Value) -> Value {
    let mut v = d.clone();
    if let Value::Object(m) = &mut v {
        m.remove("tags");
    }
    v
}
