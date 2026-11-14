//! PushTelemetry (KIP-714): a client's metrics, taken and kept, never read.

use std::time::Instant;

use bytes::Bytes;
use kafka_storage::fs::Fs;
use kafka_wire::ResponseError;
use kafka_wire::api::Response;
use kafka_wire::batch::{Compression, inflates_within};
use kafka_wire::messages::{PushTelemetryRequest, PushTelemetryResponse};

use crate::telemetry::{MAX_PAYLOAD, Verdict, subscription_id};
use crate::{Broker, Outcome};

impl<F: Fs> Broker<F> {
    /// Answered in the order Apache Kafka 4.3.1 does (recorded: `telemetry-errors`, `-throttle`, `-terminating`,
    /// `-instance-binding-spaced`): no instance id (INVALID_REQUEST); the gate, which makes an instance the broker has not
    /// seen, throttles a push that is early and refuses the one that follows a terminating push; the subscription id, then
    /// the compression type (both tell the client to ask for its subscription again); the size; the payload inflating.
    ///
    /// The payload is never read: not as OpenTelemetry, not as text. One that is not metrics is taken all the same, as a
    /// broker with no plugin to hand them to takes it. It is kept as received, bounded (see [`crate::telemetry`]), and
    /// no byte of it reaches a log. No ACL is needed (recorded: `telemetry-acl`), and any connection may speak for any
    /// instance id (recorded: `telemetry-instance-binding`, `-spaced`).
    pub(crate) fn push_telemetry(&self, req: &PushTelemetryRequest) -> Outcome {
        let mut resp = PushTelemetryResponse::default();
        resp.error_code = self.pushed(req).map_or(0, |e| e.code());
        Outcome::Respond(Response::PushTelemetry(resp))
    }

    fn pushed(&self, req: &PushTelemetryRequest) -> Option<ResponseError> {
        if req.client_instance_id.is_nil() {
            return Some(ResponseError::InvalidRequest);
        }
        let id = *req.client_instance_id.as_bytes();
        let now = Instant::now();
        match self.telemetry.arrive(id, req.terminating, now) {
            Verdict::Early => return Some(ResponseError::ThrottlingQuotaExceeded),
            Verdict::AfterTerminating => return Some(ResponseError::InvalidRequest),
            Verdict::Accepted => {}
        }
        if req.subscription_id != subscription_id(&id) {
            self.telemetry.ask_again(&id);
            return Some(ResponseError::UnknownSubscriptionId);
        }
        let Some(codec) = codec_of(req.compression_type) else {
            self.telemetry.ask_again(&id);
            return Some(ResponseError::UnsupportedCompressionType);
        };
        if req.metrics.len() > MAX_PAYLOAD {
            return Some(ResponseError::TelemetryTooLarge);
        }
        if !inflates_within(codec, &req.metrics, MAX_PAYLOAD as u64) {
            return Some(ResponseError::InvalidRecord);
        }
        // A copy, so that what is held is the payload and not the frame it arrived in.
        self.telemetry
            .keep(id, Bytes::copy_from_slice(&req.metrics), now);
        None
    }
}

/// The codec a push names: the record batch numbers, 0 (none) to 4 (zstd).
fn codec_of(code: i8) -> Option<Compression> {
    match code {
        0 => Some(Compression::None),
        1 => Some(Compression::Gzip),
        2 => Some(Compression::Snappy),
        3 => Some(Compression::Lz4),
        4 => Some(Compression::Zstd),
        _ => None,
    }
}
