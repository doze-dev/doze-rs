//! EndTxn: commit or abort.

use kafka_auth::authz::Operation;
use kafka_meta::TxnVersion;
use kafka_storage::fs::Fs;
use kafka_wire::ResponseError as E;
use kafka_wire::api::Response;
use kafka_wire::messages::{EndTxnRequest, EndTxnResponse, ProducerId};

use crate::{Broker, Caller, Outcome};

/// The first version of EndTxn of KIP-890's second part: the end raises the epoch, and the answer says the producer id and
/// epoch to go on with (`TransactionVersion.transactionVersionForEndTxn`: above v4, `TV_2`).
const RAISES_THE_EPOCH: i16 = 5;

impl<F: Fs> Broker<F> {
    /// The caller must be allowed to WRITE the transactional id (53 if not), before anything of the transaction is looked at.
    /// From v5 a success carries the producer id and epoch the producer is to go on with; a refusal carries -1 and -1.
    pub(crate) async fn end_txn_request(
        &self,
        version: i16,
        caller: &Caller,
        req: &EndTxnRequest,
    ) -> Outcome {
        let mut resp = EndTxnResponse::default();
        resp.producer_id = ProducerId(-1);
        resp.producer_epoch = -1;
        let id = req.transactional_id.0.as_str();
        let protocol = if version >= RAISES_THE_EPOCH {
            TxnVersion::EpochBump
        } else {
            TxnVersion::Classic
        };
        let answer = if !self.guard(caller).txn(Operation::Write, id) {
            Err(E::TransactionalIdAuthorizationFailed)
        } else if id.is_empty() {
            Err(E::InvalidRequest)
        } else {
            let who = (req.producer_id.0, req.producer_epoch);
            self.end_txn(id, who, req.committed, protocol).await
        };
        match answer {
            Ok((producer, epoch)) if version >= RAISES_THE_EPOCH => {
                resp.producer_id = ProducerId(producer);
                resp.producer_epoch = epoch;
            }
            Ok(_) => {}
            Err(code) => resp.error_code = crate::txn::rules::fenced_as(code, version, 2).code(),
        }
        Outcome::Respond(Response::EndTxn(resp))
    }
}
