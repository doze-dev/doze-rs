//! Subscription filters deliver. After `PutLogEvents` is answered, the lines that match each of the group's subscription filters go to
//! the filter's destination through the peer bus, in the envelope AWS sends: gzip-compressed JSON with `messageType`, `owner`,
//! `logGroup`, `logStream`, `subscriptionFilters` and `logEvents[{id, timestamp, message}]`, the ids 56-digit decimals. A Lambda function
//! is invoked asynchronously with it base64-encoded under `awslogs.data` (what a function written against AWS gunzips), the group as
//! its `SourceArn`; a Kinesis stream receives the raw gzip bytes as one record, keyed by the log stream under `distribution:
//! ByLogStream` and randomly otherwise. Putting a filter on a Kinesis stream first sends it the control message AWS sends to check the
//! destination. Delivery is one attempt; a failure is logged. Firehose and cross-account destinations are stored and not delivered.
//!
//! The Go edition delivered the same way; this one also sends the control message, and does not refuse a function subscribing to its
//! own `/aws/lambda/` group (AWS allows it; the loop it makes is the function's, as on AWS). The envelope's layout is from AWS's
//! documentation and memory; no recording exists.

use std::io::Write;

use aws_wire::service::Call;
use flate2::Compression;
use flate2::write::GzEncoder;
use serde_json::{Value, json};

use crate::core::Core;

/// One delivery, made after the call is answered.
#[derive(Debug)]
pub(crate) struct Forward {
    pub(crate) destination: String,
    /// The gzip-compressed envelope.
    pub(crate) payload: Vec<u8>,
    /// The Kinesis partition key.
    pub(crate) partition: String,
    /// The log group's ARN, the function's `SourceArn`.
    pub(crate) source_arn: String,
}

/// A line to forward.
pub(crate) struct Line<'a> {
    pub(crate) ts: i64,
    pub(crate) message: &'a str,
}

/// What a batch is from.
pub(crate) struct Origin<'a> {
    pub(crate) owner: &'a str,
    pub(crate) group: &'a str,
    pub(crate) stream: &'a str,
    pub(crate) group_arn: &'a str,
}

impl Core {
    /// Queues, for each subscription filter of the group, the lines of `lines` it matches.
    pub(crate) fn queue_forwards(&mut self, origin: &Origin<'_>, lines: &[Line<'_>]) {
        let Some(g) = self.groups.get(origin.group) else {
            return;
        };
        let mut out = Vec::new();
        for f in &g.subscriptions {
            let Ok(pattern) = crate::pattern::parse(&f.pattern) else {
                continue;
            };
            let matched: Vec<&Line<'_>> = lines
                .iter()
                .filter(|l| pattern.matches(l.message))
                .collect();
            if matched.is_empty() {
                continue;
            }
            let events: Vec<Value> = matched
                .iter()
                .enumerate()
                .map(|(n, l)| json!({"id": event_id(l.ts, n), "timestamp": l.ts, "message": l.message}))
                .collect();
            let envelope = json!({
                "messageType": "DATA_MESSAGE",
                "owner": origin.owner,
                "logGroup": origin.group,
                "logStream": origin.stream,
                "subscriptionFilters": [f.name],
                "logEvents": events,
            });
            out.push(Forward {
                destination: f.destination.clone(),
                payload: gzip(envelope.to_string().as_bytes()),
                partition: partition(f.distribution.as_deref(), origin.stream),
                source_arn: origin.group_arn.to_owned(),
            });
        }
        self.forward.extend(out);
    }

    /// Queues the control message AWS sends a Kinesis destination when a filter is put on it. (memory)
    pub(crate) fn queue_control(&mut self, origin: &Origin<'_>, destination: &str, now: i64) {
        if !destination.contains(":kinesis:") {
            return;
        }
        let envelope = json!({
            "messageType": "CONTROL_MESSAGE",
            "owner": "CloudwatchLogs",
            "logGroup": "",
            "logStream": "",
            "subscriptionFilters": [],
            "logEvents": [{"id": event_id(now, 0), "timestamp": now, "message": "CWL CONTROL MESSAGE: Checking health of destination Kinesis stream."}],
        });
        self.forward.push(Forward {
            destination: destination.to_owned(),
            payload: gzip(envelope.to_string().as_bytes()),
            partition: partition(None, origin.stream),
            source_arn: origin.group_arn.to_owned(),
        });
    }

    /// Delivers what the last call queued. A destination that is not there, or refuses, is logged and does not fail the call.
    pub(crate) async fn send_forwards(&mut self, call: &Call) {
        let queued = std::mem::take(&mut self.forward);
        let Some(peers) = self.peers.clone() else {
            return;
        };
        for f in queued {
            let parts: Vec<&str> = f.destination.splitn(6, ':').collect();
            let (service, region, account, resource) = match parts.as_slice() {
                [_, _, s, r, a, res] => (*s, *r, *a, *res),
                _ => continue,
            };
            let mut caller = call.caller.clone();
            caller.region = region.to_owned();
            caller.account_id = account.to_owned();
            let sent = match service {
                "lambda" => {
                    let event = json!({"awslogs": {"data": aws_core::base64::encode(&f.payload)}});
                    let body = json!({
                        "FunctionName": f.destination,
                        "InvocationType": "Event",
                        "Payload": aws_core::base64::encode(event.to_string().as_bytes()),
                        "SourceArn": f.source_arn,
                    });
                    peers
                        .call_rest("logs", &caller, "lambda", "Invoke", &body)
                        .await
                        .map(drop)
                }
                "kinesis" => {
                    let stream = resource.strip_prefix("stream/").unwrap_or(resource);
                    let body = json!({"StreamName": stream, "Data": aws_core::base64::encode(&f.payload), "PartitionKey": f.partition});
                    peers
                        .call("logs", &caller, "kinesis", "PutRecord", &body)
                        .await
                        .map(drop)
                }
                _ => {
                    tracing::debug!(destination = %f.destination, "logs: this destination is stored and not delivered locally");
                    continue;
                }
            };
            if let Err(e) = sent {
                tracing::warn!(destination = %f.destination, error = %e.message, "logs: a subscription filter could not deliver");
            }
        }
    }
}

/// A 56-digit decimal event id, as AWS gives: the time and the line's place in the batch.
fn event_id(ts: i64, n: usize) -> String {
    format!("{:019}{:037}", ts.max(0), n)
}

/// The Kinesis partition key: the log stream under `ByLogStream`, otherwise random.
fn partition(distribution: Option<&str>, stream: &str) -> String {
    if distribution == Some("ByLogStream") {
        return stream.to_owned();
    }
    aws_seal::alphanumeric(16).unwrap_or_else(|| stream.to_owned())
}

fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut e = GzEncoder::new(Vec::new(), Compression::default());
    if e.write_all(bytes).is_err() {
        return Vec::new();
    }
    e.finish().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use flate2::read::GzDecoder;

    use super::*;

    #[test]
    fn ids_are_56_digits_and_the_payload_gunzips() {
        assert_eq!(event_id(1_700_000_000_000, 3).len(), 56);
        let packed = gzip(br#"{"a":1}"#);
        let mut out = String::new();
        GzDecoder::new(packed.as_slice())
            .read_to_string(&mut out)
            .unwrap_or_default();
        assert_eq!(out, r#"{"a":1}"#);
        assert_eq!(partition(Some("ByLogStream"), "s"), "s");
        assert_eq!(partition(Some("Random"), "s").len(), 16);
    }
}
