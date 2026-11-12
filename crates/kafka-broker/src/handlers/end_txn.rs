//! EndTxn: commit or abort.

use kafka_auth::authz::operation;
use kafka_storage::fs::Fs;
use kafka_wire::api::Response;
use kafka_wire::messages::{EndTxnRequest, EndTxnResponse};

use crate::{Broker, Caller, Outcome};

impl<F: Fs> Broker<F> {
    /// The caller must be allowed to WRITE the transactional id (53 if not), before anything of the transaction is looked at.
    pub(crate) async fn end_txn_request(
        &self,
        version: i16,
        caller: &Caller,
        req: &EndTxnRequest,
    ) -> Outcome {
        let mut resp = EndTxnResponse::default();
        let id = req.transactional_id.0.as_str();
        resp.error_code = if !self.guard(caller).txn(operation::WRITE, id) {
            kafka_wire::ResponseError::TransactionalIdAuthorizationFailed.code()
        } else if id.is_empty() {
            kafka_wire::ResponseError::InvalidRequest.code()
        } else {
            self.end_txn(id, req.producer_id.0, req.producer_epoch, req.committed)
                .await
                .map_or(0, |e| crate::txn::rules::fenced_as(e, version, 2).code())
        };
        Outcome::Respond(Response::EndTxn(resp))
    }
}
