//! InitProducerId.

use kafka_storage::fs::Fs;
use kafka_wire::ResponseError;
use kafka_wire::messages::{InitProducerIdRequest, InitProducerIdResponse, ProducerId};

use crate::Broker;

impl<F: Fs> Broker<F> {
    /// Hands out a fresh producer id (epoch 0). Producers default to idempotence and ask for one before they
    /// produce anything, so refusing puts a client into a fatal state; what an idempotent producer's id is used
    /// for comes with the sequence checks (Phase 6). A transactional id is refused until the transaction APIs
    /// exist: none of them is advertised, so a client that follows the advertised APIs never sends one.
    pub(crate) fn init_producer_id(&self, req: &InitProducerIdRequest) -> InitProducerIdResponse {
        let mut resp = InitProducerIdResponse::default();
        if req.transactional_id.is_some() {
            resp.error_code = ResponseError::InvalidRequest.code();
            resp.producer_id = ProducerId(-1);
            resp.producer_epoch = -1;
            return resp;
        }
        match self.meta.next_producer_id() {
            Ok(id) => {
                resp.producer_id = ProducerId(id);
                resp.producer_epoch = 0;
            }
            Err(e) => {
                tracing::error!(error = %e, "reserving producer ids failed");
                resp.error_code = ResponseError::KafkaStorageError.code();
                resp.producer_id = ProducerId(-1);
                resp.producer_epoch = -1;
            }
        }
        resp
    }
}
