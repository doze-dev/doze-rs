//! What an SNS publish did, as the bus saw it: a `Publish` answers with a message id and nothing of who got it, so how many
//! deliveries it made is counted from the calls SNS made to queues and functions while it answered ([`aws_peers::seen`] marks
//! each with the request it was inside of). Those calls are recorded before the publish is, so they are already in the ring.
//!
//! This counts deliveries over the bus: a queue or a function. An `http` or `https` endpoint is posted to off the bus and an
//! `email`, `sms` or `application` subscription is stored and sent nowhere, so none of those is counted.

use aws_gateway::Finished;
use aws_peers::summary::{Kind, Summary};

use super::Recorder;

/// The service a delivery names itself as ([`super::Via::by`]).
const SNS: &str = "sns";

impl Recorder {
    /// The summary of `call`: as the gateway read it, except a publish, whose deliveries are counted here.
    pub(super) fn deliveries(&self, call: &Finished<'_>) -> Summary {
        let summary = call.did.summary;
        if summary.kind != Kind::Delivered {
            return summary;
        }
        let ring = self
            .ring
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (mut ok, mut failed) = (0u32, 0u16);
        for held in ring.calls.iter().rev() {
            let Some(via) = held.via.as_deref() else {
                continue;
            };
            if &*via.request != call.request_id || &*via.by != SNS {
                continue;
            }
            if held.status < 400 {
                ok += 1;
            } else {
                failed = failed.saturating_add(1);
            }
        }
        Summary::of(Kind::Delivered, ok, failed)
    }
}
