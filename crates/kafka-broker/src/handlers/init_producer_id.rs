//! InitProducerId.

use kafka_auth::authz::{CLUSTER_NAME, operation, resource};
use kafka_storage::fs::Fs;
use kafka_wire::ResponseError;
use kafka_wire::messages::{InitProducerIdRequest, InitProducerIdResponse, ProducerId};

use crate::txn::rules;
use crate::{Broker, Caller, blocking};

/// The longest transactional id kept (it is kept for good, in memory and on disk).
const MOST_BYTES: usize = 4096;

impl<F: Fs> Broker<F> {
    /// Hands out a producer id (epoch 0). Producers default to idempotence and ask for one before they produce
    /// anything, so refusing puts a client into a fatal state. With a transactional id the caller becomes the owner of
    /// that id's transactions, fencing whoever held it.
    ///
    /// Before anything else is looked at, the caller must be allowed: to WRITE the transactional id (53 if not); or,
    /// with no id, to IDEMPOTENT_WRITE on the cluster or WRITE to at least one topic (31 if not). The second is
    /// Kafka's rule since KIP-679: a producer that may write somewhere may be given a producer id.
    pub(crate) async fn init_producer_id(
        &self,
        version: i16,
        caller: &Caller,
        req: &InitProducerIdRequest,
    ) -> InitProducerIdResponse {
        let mut resp = InitProducerIdResponse::default();
        let refuse = |mut resp: InitProducerIdResponse, code: ResponseError| {
            resp.error_code = code.code();
            resp.producer_id = ProducerId(-1);
            resp.producer_epoch = -1;
            resp
        };
        if let Some(code) = self.refusal_for(caller, req) {
            return refuse(resp, code);
        }
        if let Some(id) = &req.transactional_id {
            // An empty id is not "no id" (that is null).
            if id.0.is_empty() || id.0.len() > MOST_BYTES {
                return refuse(resp, ResponseError::InvalidRequest);
            }
            // A producer that says which producer id it holds must say its epoch too (KIP-360, version 3).
            let expected = match (req.producer_id.0, req.producer_epoch) {
                (-1, -1) => None,
                (-1, _) | (_, -1) => return refuse(resp, ResponseError::InvalidRequest),
                (producer, epoch) => Some((producer, epoch)),
            };
            return match self
                .init_txn_producer(id.0.as_str(), req.transaction_timeout_ms, expected)
                .await
            {
                Ok((producer, epoch)) => {
                    resp.producer_id = ProducerId(producer);
                    resp.producer_epoch = epoch;
                    resp
                }
                Err(code) => refuse(resp, rules::fenced_as(code, version, 4)),
            };
        }
        match blocking::run(|| self.meta.next_producer_id()) {
            Ok(id) => {
                resp.producer_id = ProducerId(id);
                resp.producer_epoch = 0;
                resp
            }
            Err(e) => {
                tracing::error!(error = %e, "reserving producer ids failed");
                refuse(resp, ResponseError::KafkaStorageError)
            }
        }
    }

    /// Why the caller may not be given a producer id, if it may not.
    fn refusal_for(&self, caller: &Caller, req: &InitProducerIdRequest) -> Option<ResponseError> {
        match &req.transactional_id {
            Some(id) => (!self.authz.allows(
                caller,
                resource::TRANSACTIONAL_ID,
                id.0.as_str(),
                operation::WRITE,
            ))
            .then_some(ResponseError::TransactionalIdAuthorizationFailed),
            None => (!self.authz.allows(
                caller,
                resource::CLUSTER,
                CLUSTER_NAME,
                operation::IDEMPOTENT_WRITE,
            ) && !self
                .authz
                .allows_some(caller, operation::WRITE, resource::TOPIC))
            .then_some(ResponseError::ClusterAuthorizationFailed),
        }
    }
}
