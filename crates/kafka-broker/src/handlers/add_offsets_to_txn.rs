//! AddOffsetsToTxn: the consumer group whose offsets the transaction will commit.

use kafka_auth::authz::Operation;
use kafka_meta::TxnVersion;
use kafka_storage::fs::Fs;
use kafka_wire::ResponseError as E;
use kafka_wire::api::Response;
use kafka_wire::messages::{AddOffsetsToTxnRequest, AddOffsetsToTxnResponse};

use crate::txn::rules;
use crate::{Broker, Caller, Outcome};

impl<F: Fs> Broker<F> {
    /// The caller must be allowed to WRITE the transactional id (53), then to READ the group (30): both before the
    /// transaction is looked at.
    pub(crate) async fn add_offsets_to_txn(
        &self,
        version: i16,
        caller: &Caller,
        req: &AddOffsetsToTxnRequest,
    ) -> Outcome {
        let guard = self.guard(caller);
        let mut resp = AddOffsetsToTxnResponse::default();
        let refused = if !guard.txn(Operation::Write, req.transactional_id.0.as_str()) {
            Some(E::TransactionalIdAuthorizationFailed)
        } else if !guard.group(Operation::Read, req.group_id.0.as_str()) {
            Some(E::GroupAuthorizationFailed)
        } else {
            None
        };
        resp.error_code = match refused {
            Some(e) => e.code(),
            None => self
                .add_offsets(req)
                .await
                .map_or(0, |e| rules::fenced_as(e, version, 2).code()),
        };
        Outcome::Respond(Response::AddOffsetsToTxn(resp))
    }

    async fn add_offsets(&self, req: &AddOffsetsToTxnRequest) -> Option<E> {
        let id = req.transactional_id.0.as_str();
        if id.is_empty() {
            return Some(E::InvalidRequest);
        }
        // The group id is not checked: Kafka adds an empty one to the transaction and answers 0.
        let Some(mut e) = self.txns.lock(id).await else {
            return Some(E::InvalidProducerIdMapping);
        };
        let group = req.group_id.0.as_str();
        let there = rules::has_group(&e.rec, group);
        match rules::may_add(&e.rec, req.producer_id.0, req.producer_epoch, there) {
            rules::Adding::Refused(code) => Some(code),
            rules::Adding::There => None,
            rules::Adding::Store => {
                // At the classic transaction version whatever the request's (KafkaApis: "This request will always come
                // from the client not using TV 2").
                let now = crate::clock::now().get();
                self.update_txn(&mut e, |rec| {
                    rules::add_group(rec, group, TxnVersion::Classic, now);
                })
                .await
                .err()
            }
        }
    }
}
