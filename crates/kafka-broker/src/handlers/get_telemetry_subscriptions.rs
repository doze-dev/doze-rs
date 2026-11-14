//! GetTelemetrySubscriptions (KIP-714): what a client is asked to report, and how often.

use std::time::Instant;

use kafka_storage::fs::Fs;
use kafka_wire::api::Response;
use kafka_wire::messages::{GetTelemetrySubscriptionsRequest, GetTelemetrySubscriptionsResponse};
use kafka_wire::{ResponseError, StrBytes, Uuid};

use crate::telemetry::{
    ACCEPTED_COMPRESSION, MAX_PAYLOAD, PUSH_INTERVAL, REQUESTED_METRICS, Verdict, subscription_id,
};
use crate::{Broker, Outcome};

impl<F: Fs> Broker<F> {
    /// The same subscription for every client: the standard metric set, every five seconds, cumulative. Kafka subscribes a
    /// client to nothing until an operator defines a CLIENT_METRICS resource, and this broker has none to define, so the
    /// subscription is fixed (the replayed recordings define the same five seconds on Kafka).
    ///
    /// A client with no instance id is given one. No ACL is needed (recorded: `telemetry-acl`, a user with no ACLs at all
    /// is answered). A client that asks again before the interval has passed is THROTTLING_QUOTA_EXCEEDED, with nothing else
    /// in the answer.
    pub(crate) fn get_telemetry_subscriptions(
        &self,
        req: &GetTelemetrySubscriptionsRequest,
    ) -> Outcome {
        let mut resp = GetTelemetrySubscriptionsResponse::default();
        let id = if req.client_instance_id.is_nil() {
            new_instance_id()
        } else {
            *req.client_instance_id.as_bytes()
        };
        if self.telemetry.subscribe(id, Instant::now()) == Verdict::Early {
            resp.error_code = ResponseError::ThrottlingQuotaExceeded.code();
            return Outcome::Respond(Response::GetTelemetrySubscriptions(resp));
        }
        resp.client_instance_id = Uuid::from_bytes(id);
        resp.subscription_id = subscription_id(&id);
        resp.accepted_compression_types = ACCEPTED_COMPRESSION.to_vec();
        resp.push_interval_ms = i32::try_from(PUSH_INTERVAL.as_millis()).unwrap_or(i32::MAX);
        resp.telemetry_max_bytes = i32::try_from(MAX_PAYLOAD).unwrap_or(i32::MAX);
        resp.delta_temporality = false;
        resp.requested_metrics = vec![StrBytes::from_static_str(REQUESTED_METRICS)];
        Outcome::Respond(Response::GetTelemetrySubscriptions(resp))
    }
}

/// A fresh instance id: a version 4 UUID, as the broker assigns to a client that has none.
fn new_instance_id() -> [u8; 16] {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let mut bytes = [0_u8; 16];
    if getrandom::fill(&mut bytes).is_err() {
        // Without randomness two ids must still differ: the time and a count will do for a local broker.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        bytes[..8].copy_from_slice(&COUNTER.fetch_add(1, Ordering::Relaxed).to_be_bytes());
        bytes[8..].copy_from_slice(&now.to_be_bytes()[8..]);
    }
    // Version 4, variant 10: the fixed bits of a random UUID.
    if let Some(b) = bytes.get_mut(6) {
        *b = (*b & 0x0f) | 0x40;
    }
    if let Some(b) = bytes.get_mut(8) {
        *b = (*b & 0x3f) | 0x80;
    }
    bytes
}
