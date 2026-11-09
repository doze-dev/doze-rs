//! EndTxn: commit or abort.

use kafka_storage::fs::Fs;
use kafka_wire::api::Response;
use kafka_wire::messages::{EndTxnRequest, EndTxnResponse};

use crate::{Broker, Outcome};

impl<F: Fs> Broker<F> {
    pub(crate) async fn end_txn_request(&self, req: &EndTxnRequest) -> Outcome {
        let mut resp = EndTxnResponse::default();
        let id = req.transactional_id.0.as_str();
        resp.error_code = if id.is_empty() {
            kafka_wire::ResponseError::InvalidRequest.code()
        } else {
            self.end_txn(id, req.producer_id.0, req.producer_epoch, req.committed)
                .await
                .map_or(0, |e| e.code())
        };
        Outcome::Respond(Response::EndTxn(resp))
    }
}
