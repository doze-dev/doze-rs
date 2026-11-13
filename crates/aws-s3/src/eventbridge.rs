//! Bucket events to EventBridge. A bucket whose notification configuration has an `EventBridgeConfiguration` puts every event on the
//! account's default bus, as source `aws.s3`: `Object Created`, `Object Deleted`, `Object Tags Added` and `Object Tags Deleted`, with
//! the `detail` AWS documents (bucket, object, request id, requester, reason). The test event of a notification configuration goes to
//! queues, topics and functions and not to EventBridge. Events the Go edition never raised (per-key deletes, delete markers, lifecycle
//! expiry) are raised here too.
//!
//! The names and fields are from AWS's documentation of S3 events on EventBridge, as remembered; no recording of one exists.

use aws_eventbridge::publish::{self, Event as Put};
use serde_json::{Value, json};

use crate::core::Core;
use crate::events::{Event, Kind};

/// What an S3 event name becomes: the `detail-type`, the `reason`, and for a removal the `deletion-type`.
fn translate(name: &str) -> Option<(&'static str, &'static str, Option<&'static str>)> {
    Some(match name {
        "ObjectCreated:Put" => ("Object Created", "PutObject", None),
        "ObjectCreated:Post" => ("Object Created", "POST Object", None),
        "ObjectCreated:Copy" => ("Object Created", "CopyObject", None),
        "ObjectCreated:CompleteMultipartUpload" => {
            ("Object Created", "CompleteMultipartUpload", None)
        }
        "ObjectRemoved:Delete" => (
            "Object Deleted",
            "DeleteObject",
            Some("Permanently Deleted"),
        ),
        "ObjectRemoved:DeleteMarkerCreated" => (
            "Object Deleted",
            "DeleteObject",
            Some("Delete Marker Created"),
        ),
        "LifecycleExpiration:Delete" => (
            "Object Deleted",
            "Lifecycle Expiration",
            Some("Permanently Deleted"),
        ),
        "LifecycleExpiration:DeleteMarkerCreated" => (
            "Object Deleted",
            "Lifecycle Expiration",
            Some("Delete Marker Created"),
        ),
        "ObjectTagging:Put" => ("Object Tags Added", "PutObjectTagging", None),
        "ObjectTagging:Delete" => ("Object Tags Deleted", "DeleteObjectTagging", None),
        _ => return None,
    })
}

/// The `detail` of an event, and its `detail-type`.
fn detail(event: &Event) -> Option<(&'static str, Value)> {
    let Kind::Object(name) = &event.kind else {
        return None;
    };
    let (detail_type, reason, deletion) = translate(name)?;
    let mut object = json!({"key": event.key, "sequencer": format!("{:016X}", event.at_ms)});
    if let Some(map) = object.as_object_mut() {
        if detail_type == "Object Created" {
            map.insert("size".into(), json!(event.size));
        }
        if !event.etag.is_empty() {
            map.insert("etag".into(), json!(event.etag));
        }
        if let Some(v) = &event.version_id {
            map.insert("version-id".into(), json!(v));
        }
    }
    let mut detail = json!({
        "version": "0",
        "bucket": {"name": event.bucket},
        "object": object,
        "request-id": event.request_id,
        "requester": event.caller.account_id,
        "source-ip-address": "127.0.0.1",
    });
    if let Some(map) = detail.as_object_mut() {
        if detail_type != "Object Tags Added" && detail_type != "Object Tags Deleted" {
            map.insert("reason".into(), json!(reason));
        }
        if let Some(d) = deletion {
            map.insert("deletion-type".into(), json!(d));
        }
    }
    Some((detail_type, detail))
}

impl Core {
    /// Puts the event on the default bus. An event that cannot be (no EventBridge is running, or the bus refused it) is lost, as a
    /// notification to a queue that is gone is: the request that caused it has already succeeded.
    pub(crate) async fn to_eventbridge(&self, event: &Event) {
        let Some(peers) = &self.peers else { return };
        let Some((detail_type, detail)) = detail(event) else {
            return;
        };
        let resources = [format!("arn:aws:s3:::{}", event.bucket)];
        let put = Put {
            source: "aws.s3",
            detail_type,
            detail: &detail,
            resources: &resources,
            time_ms: Some(event.at_ms),
        };
        // WHY: the request that caused the event has succeeded; EventBridge not running, or refusing the event, is not S3's to report.
        drop(publish::put(peers, "s3", &event.caller, &put).await);
    }
}
