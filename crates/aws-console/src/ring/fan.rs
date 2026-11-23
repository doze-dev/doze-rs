//! What an SNS publish did, as the bus saw it: a `Publish` answers with a message id and nothing of who got it, so how many
//! deliveries it made is counted from the calls SNS made to queues and functions while it answered ([`aws_peers::seen`] marks
//! each with the request it was inside of). Those calls are recorded before the publish is, so they are already in the ring.
//!
//! A publish the console makes is not a request from the wire: the console gives it a request of its own ([`crate::resources::bus::Bus::call`]),
//! so its deliveries are counted the same way and its row says `delivered to 2` as an AWS CLI's does.
//!
//! This counts deliveries over the bus: a queue or a function. An `http` or `https` endpoint is posted to off the bus and an
//! `email`, `sms` or `application` subscription is stored and sent nowhere, so none of those is counted.

use aws_gateway::Finished;
use aws_peers::BusCall;
use aws_peers::summary::{Kind, Summary};

use super::Recorder;
use crate::resources::bus::is_console_request;

/// The service a delivery names itself as ([`super::Via::by`]).
const SNS: &str = "sns";

impl Recorder {
    /// The summary of `call`: as the gateway read it, except a publish, whose deliveries are counted here.
    pub(super) fn deliveries(&self, call: &Finished<'_>) -> Summary {
        let summary = call.did.summary;
        if summary.kind != Kind::Delivered {
            return summary;
        }
        self.delivered_for(call.request_id)
    }

    /// The summary of a publish the console made over the bus (`delivered to 2`), or `None` for any other bus call.
    pub(super) fn console_publish(&self, call: &BusCall<'_>) -> Option<Summary> {
        let request = call
            .cause?
            .request
            .as_deref()
            .filter(|r| is_console_request(r))?;
        let publishes =
            call.signing_name == SNS && matches!(call.operation, "Publish" | "PublishBatch");
        (publishes && call.acting.is_empty() && call.status < 400)
            .then(|| self.delivered_for(request))
    }

    /// The deliveries SNS made for `request`.
    fn delivered_for(&self, request: &str) -> Summary {
        let ring = self
            .ring
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (mut ok, mut failed) = (0u32, 0u16);
        for held in ring.calls.iter().rev() {
            let Some(via) = held.via.as_deref() else {
                continue;
            };
            if &*via.request != request || &*via.by != SNS {
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
