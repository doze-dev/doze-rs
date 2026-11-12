//! Event notifications: what a bucket sends when objects are created, removed or tagged. A call notes its events as it changes
//! things; once the change is on disk they are matched against the bucket's notification configuration and delivered. Queues are
//! delivered to through the peer bus, and so is the default event bus when the configuration asks for it (`eventbridge.rs`); topics
//! and Lambda functions are the hooks marked below, filled in when those services exist (the configuration for them is already stored
//! and returned as sent).

use aws_core::time::format_iso8601;
use aws_wire::service::Caller;
use serde_json::{Value, json};

use crate::core::Core;

/// What happened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// `ObjectCreated:Put`, `ObjectRemoved:Delete`, ...
    Object(&'static str),
    /// The `s3:TestEvent` S3 sends to a destination when a configuration is put.
    Test,
}

/// An event to deliver.
#[derive(Clone, Debug)]
pub(crate) struct Event {
    pub(crate) kind: Kind,
    pub(crate) bucket: String,
    pub(crate) key: String,
    pub(crate) size: u64,
    pub(crate) etag: String,
    pub(crate) version_id: Option<String>,
    pub(crate) at_ms: i64,
    pub(crate) request_id: String,
    pub(crate) caller: Caller,
}

impl Event {
    /// The name the configuration's event patterns are matched against: `s3:ObjectCreated:Put`.
    fn full_name(&self) -> String {
        match &self.kind {
            Kind::Object(n) => format!("s3:{n}"),
            Kind::Test => "s3:TestEvent".to_owned(),
        }
    }

    /// The message body: S3's event record (version 2.1), or the test event.
    pub(crate) fn body(&self, config_id: &str) -> Value {
        let time = format_iso8601(self.at_ms / 1000);
        let Kind::Object(name) = &self.kind else {
            return json!({
                "Service": "Amazon S3", "Event": "s3:TestEvent", "Time": time,
                "Bucket": self.bucket, "RequestId": self.request_id,
                "HostId": aws_wire::rest::response::host_id(&aws_wire::RequestId::new()),
            });
        };
        let mut object = serde_json::Map::new();
        object.insert(
            "key".into(),
            json!(aws_core::uri::encode(self.key.as_bytes(), true).replace("%20", "+")),
        );
        object.insert("sequencer".into(), json!(format!("{:016X}", self.at_ms)));
        if !name.starts_with("ObjectRemoved") {
            object.insert("size".into(), json!(self.size));
            object.insert("eTag".into(), json!(self.etag));
        }
        if let Some(v) = &self.version_id {
            object.insert("versionId".into(), json!(v));
        }
        json!({"Records": [{
            "eventVersion": "2.1", "eventSource": "aws:s3", "awsRegion": self.caller.region,
            "eventTime": time, "eventName": name,
            "userIdentity": {"principalId": format!("AWS:{}", self.caller.access_key_id)},
            "requestParameters": {"sourceIPAddress": "127.0.0.1"},
            "responseElements": {"x-amz-request-id": self.request_id, "x-amz-id-2": self.request_id},
            "s3": {
                "s3SchemaVersion": "1.0", "configurationId": config_id,
                "bucket": {
                    "name": self.bucket,
                    "ownerIdentity": {"principalId": self.caller.account_id},
                    "arn": format!("arn:aws:s3:::{}", self.bucket),
                },
                "object": object,
            },
        }]})
    }
}

/// Whether one of a configuration's event patterns (`s3:ObjectCreated:*`) covers `event`.
fn covers(patterns: &[&str], event: &str) -> bool {
    patterns.iter().any(|p| {
        p.strip_suffix('*')
            .map_or(*p == event, |stem| event.starts_with(stem))
    })
}

/// Whether the key passes the entry's `Filter` (S3 keys: `prefix` and `suffix` rules).
fn passes(entry: &Value, key: &str) -> bool {
    let rules = entry
        .pointer("/Filter/Key/FilterRules")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    rules.iter().all(|r| {
        let value = r.get("Value").and_then(Value::as_str).unwrap_or_default();
        match r
            .get("Name")
            .and_then(Value::as_str)
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("prefix") => key.starts_with(value),
            Some("suffix") => key.ends_with(value),
            _ => true,
        }
    })
}

impl Core {
    /// Notes an event for delivery once the call's changes are on disk.
    pub(crate) fn note(
        &mut self,
        req: &crate::req::Req<'_>,
        kind: Kind,
        v: &crate::model::Version,
    ) {
        self.events.push(Event {
            kind,
            bucket: req.bucket().to_owned(),
            key: v.key.clone(),
            size: v.size,
            etag: v.etag.clone(),
            version_id: Some(v.version_id.clone()).filter(|v| v != "null"),
            at_ms: self.now_ms(),
            request_id: req.call.id.as_str().to_owned(),
            caller: req.call.caller.clone(),
        });
    }

    /// Notes an event about something that is not a stored version (a removal, the test event).
    pub(crate) fn note_raw(
        &mut self,
        req: &crate::req::Req<'_>,
        kind: Kind,
        key: &str,
        size: u64,
        version: Option<&str>,
    ) {
        self.events.push(Event {
            kind,
            bucket: req.bucket().to_owned(),
            key: key.to_owned(),
            size,
            etag: String::new(),
            version_id: version.filter(|v| *v != "null").map(str::to_owned),
            at_ms: self.now_ms(),
            request_id: req.call.id.as_str().to_owned(),
            caller: req.call.caller.clone(),
        });
    }

    /// Notes the event for something a lifecycle rule did (there is no request, so the last caller's account and region stand in).
    pub(crate) fn note_lifecycle(&mut self, bucket: &str, key: &str, r: &crate::delete::Removed) {
        let Some(mut caller) = self.last_caller.clone() else {
            return;
        };
        "s3.amazonaws.com".clone_into(&mut caller.access_key_id);
        let name = if r.created {
            "LifecycleExpiration:DeleteMarkerCreated"
        } else {
            "LifecycleExpiration:Delete"
        };
        self.events.push(Event {
            kind: Kind::Object(name),
            bucket: bucket.to_owned(),
            key: key.to_owned(),
            size: r.size,
            etag: String::new(),
            version_id: r.version_id.clone().filter(|v| v != "null"),
            at_ms: self.now_ms(),
            request_id: aws_wire::RequestId::new().to_string(),
            caller,
        });
    }

    /// Delivers the events a call noted. A destination that cannot be reached loses its event, as S3 does not retry into a missing
    /// queue; the call that caused the event has already succeeded.
    pub(crate) async fn deliver(&mut self) {
        let events = std::mem::take(&mut self.events);
        for event in events {
            let Some(config) = self
                .buckets
                .get(&event.bucket)
                .and_then(|b| b.configs.get("notification"))
                .and_then(|c| serde_json::from_str::<Value>(c).ok())
            else {
                continue;
            };
            self.deliver_event(&event, &config).await;
        }
    }

    async fn deliver_event(&self, event: &Event, config: &Value) {
        let name = event.full_name();
        for (list, arn_member) in [
            ("QueueConfigurations", "QueueArn"),
            ("TopicConfigurations", "TopicArn"),
            ("LambdaFunctionConfigurations", "LambdaFunctionArn"),
        ] {
            let entries = config
                .get(list)
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default();
            for entry in entries {
                let patterns: Vec<&str> = entry
                    .get("Events")
                    .and_then(Value::as_array)
                    .map(|e| e.iter().filter_map(Value::as_str).collect())
                    .unwrap_or_default();
                // A test event goes to every destination; a real one to those whose events and filter fit it.
                let fits = event.kind == Kind::Test
                    || (covers(&patterns, &name) && passes(entry, &event.key));
                if !fits {
                    continue;
                }
                let arn = entry
                    .get(arn_member)
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let id = entry.get("Id").and_then(Value::as_str).unwrap_or_default();
                match list {
                    "QueueConfigurations" => self.to_queue(event, arn, id).await,
                    // Hook for SNS (stage A8): publish `event.body(id)` to the topic `arn`.
                    "TopicConfigurations" => {}
                    // Hook for Lambda (stage A13): invoke the function `arn` asynchronously with `event.body(id)`.
                    _ => {}
                }
            }
        }
        // A configuration with an `EventBridgeConfiguration` puts every event on the account's default bus (SCOPE.md item 12).
        if config.get("EventBridgeConfiguration").is_some() {
            self.to_eventbridge(event).await;
        }
    }

    async fn to_queue(&self, event: &Event, arn: &str, config_id: &str) {
        let Some(peers) = &self.peers else { return };
        let name = arn.rsplit(':').next().unwrap_or_default();
        let asked = peers
            .call(
                "s3",
                &event.caller,
                "sqs",
                "GetQueueUrl",
                &json!({"QueueName": name}),
            )
            .await;
        let Some(url) = asked
            .ok()
            .and_then(|v| v.get("QueueUrl").and_then(Value::as_str).map(str::to_owned))
        else {
            return;
        };
        let body = event.body(config_id).to_string();
        // WHY: the queue may have been deleted since the configuration was put; S3 does not fail the request that caused the event.
        peers
            .call(
                "s3",
                &event.caller,
                "sqs",
                "SendMessage",
                &json!({"QueueUrl": url, "MessageBody": body}),
            )
            .await
            .ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns_and_filters_decide_who_is_told() {
        assert!(covers(&["s3:ObjectCreated:*"], "s3:ObjectCreated:Put"));
        assert!(covers(&["s3:ObjectCreated:Put"], "s3:ObjectCreated:Put"));
        assert!(!covers(&["s3:ObjectCreated:Put"], "s3:ObjectCreated:Copy"));
        assert!(!covers(&["s3:ObjectRemoved:*"], "s3:ObjectCreated:Put"));
        assert!(covers(&["s3:*"], "s3:ObjectRemoved:Delete"));
        let entry = json!({"Filter": {"Key": {"FilterRules": [{"Name": "Prefix", "Value": "in/"}, {"Name": "suffix", "Value": ".csv"}]}}});
        assert!(passes(&entry, "in/a.csv"));
        assert!(!passes(&entry, "in/a.txt"));
        assert!(!passes(&entry, "out/a.csv"));
    }
}
