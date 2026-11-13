//! Bucket notifications: checking a configuration when it is put. Queue destinations are checked to exist (and not to be FIFO), as
//! S3 checks them; topic and function destinations are taken as named. Delivery to all three (and to EventBridge) is `events.rs`.

use aws_wire::ApiError;
use serde_json::{Value, json};

use crate::core::Core;
use crate::errors::{invalid_argument, malformed_xml};
use crate::req::Req;

/// The notification kinds and the member naming each one's destination.
const TARGETS: [(&str, &str); 3] = [
    ("QueueConfigurations", "QueueArn"),
    ("TopicConfigurations", "TopicArn"),
    ("LambdaFunctionConfigurations", "LambdaFunctionArn"),
];

fn new_id() -> String {
    let mut b = [0u8; 18];
    // WHY: without randomness the id is the zero string; ids only need to differ within a configuration.
    if getrandom::fill(&mut b).is_err() {
        b.fill(0);
    }
    aws_core::base64::encode(&b).replace(['+', '/'], "x")
}

impl Core {
    /// Checks a notification configuration: every entry has events and a destination, the destination queues exist, and entries
    /// without an id get one.
    pub(crate) async fn check_notification(
        &self,
        req: &Req<'_>,
        body: &mut Value,
    ) -> Result<(), ApiError> {
        for (list, arn_member) in TARGETS {
            let Some(entries) = body.get_mut(list).and_then(Value::as_array_mut) else {
                continue;
            };
            for entry in entries {
                let arn = entry
                    .get(arn_member)
                    .and_then(Value::as_str)
                    .ok_or_else(malformed_xml)?
                    .to_owned();
                let events_ok = entry
                    .get("Events")
                    .and_then(Value::as_array)
                    .is_some_and(|e| {
                        !e.is_empty()
                            && e.iter()
                                .all(|x| x.as_str().is_some_and(|s| s.starts_with("s3:")))
                    });
                if !events_ok {
                    return Err(malformed_xml());
                }
                if list == "QueueConfigurations" {
                    self.check_queue(req, &arn).await?;
                }
                if let Some(o) = entry.as_object_mut()
                    && !o.contains_key("Id")
                {
                    o.insert("Id".into(), json!(new_id()));
                }
            }
        }
        Ok(())
    }

    /// A queue destination must be a standard queue that exists.
    async fn check_queue(&self, req: &Req<'_>, arn: &str) -> Result<(), ApiError> {
        let name = arn.rsplit(':').next().unwrap_or_default();
        if name.ends_with(".fifo") {
            return Err(invalid_argument("FIFO SQS queues are not supported."));
        }
        let Some(peers) = &self.peers else {
            return Ok(());
        };
        let asked = peers
            .call(
                "s3",
                &req.call.caller,
                "sqs",
                "GetQueueUrl",
                &json!({"QueueName": name}),
            )
            .await;
        match asked {
            Ok(_) => Ok(()),
            // (memory) the wording of S3's refusal of a destination it cannot validate.
            Err(_) => Err(invalid_argument(
                "Unable to validate the following destination configurations",
            )),
        }
    }
}
