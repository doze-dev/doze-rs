//! FindCoordinator: this broker, for every group and every transaction.

use kafka_auth::authz::operation;
use kafka_storage::fs::Fs;
use kafka_wire::ResponseError;
use kafka_wire::StrBytes;
use kafka_wire::messages::find_coordinator_response::Coordinator;
use kafka_wire::messages::{BrokerId, FindCoordinatorRequest, FindCoordinatorResponse};

use crate::{Broker, Caller, Listener};

/// Kafka's words for a request it cannot make sense of.
const INVALID_REQUEST: &str = "This most likely occurs because of a request being malformed by the client library or the message was sent to an incompatible broker. See the broker logs for more details.";

/// A group's coordinator is a key type 0, a transaction's 1; share groups (2) are not offered.
const GROUP: i8 = 0;
const TRANSACTION: i8 = 1;

/// The words of a refusal before v4, which Kafka takes from the error itself (`prepareOldResponse`).
const GROUP_DENIED: &str = "Group authorization failed.";
const TXN_DENIED: &str = "Transactional Id authorization failed.";

impl<F: Fs> Broker<F> {
    /// A single node is its own coordinator. Versions 0 to 3 ask for one key and v4 and later for a list; both are
    /// answered here, the answer to a key type that is no coordinator's being INVALID_REQUEST with no broker.
    ///
    /// Before any of that, the caller must be allowed to DESCRIBE the group (key type 0) or the transactional id
    /// (key type 1): a refusal is 30 or 53, with no broker, for the key (v4 and later) or at the top (before).
    pub(crate) fn find_coordinator(
        &self,
        version: i16,
        at: &Listener,
        caller: &Caller,
        req: &FindCoordinatorRequest,
    ) -> FindCoordinatorResponse {
        let guard = self.guard(caller);
        let refused = |key: &str| match req.key_type {
            GROUP if !guard.group(operation::DESCRIBE, key) => {
                Some(ResponseError::GroupAuthorizationFailed)
            }
            TRANSACTION if !guard.txn(operation::DESCRIBE, key) => {
                Some(ResponseError::TransactionalIdAuthorizationFailed)
            }
            _ => None,
        };
        let known = matches!(req.key_type, GROUP | TRANSACTION) || version == 0;
        let me = |c: &mut Coordinator| {
            c.node_id = BrokerId(self.config.node_id);
            c.host = StrBytes::from_string(at.advertised_host.clone());
            c.port = at.advertised_port;
        };
        let mut resp = FindCoordinatorResponse::default();
        // v0 to v3: one coordinator in the response itself (a later version has none there: the fields are not
        // in the message, and setting them would stop it being written). A success reads "NONE" from v1 to v3, as
        // Kafka puts it.
        if version < 4 {
            if let Some(error) = refused(req.key.as_str()) {
                resp.error_code = error.code();
                if version >= 1 {
                    resp.error_message = Some(StrBytes::from_static_str(denied_words(error)));
                }
                resp.node_id = BrokerId(-1);
                resp.port = -1;
            } else if known {
                resp.node_id = BrokerId(self.config.node_id);
                resp.host = StrBytes::from_string(at.advertised_host.clone());
                resp.port = at.advertised_port;
                if (1..=3).contains(&version) {
                    resp.error_message = Some(StrBytes::from_static_str("NONE"));
                }
            } else {
                resp.error_code = ResponseError::InvalidRequest.code();
                resp.error_message = invalid_message(req.key_type);
                resp.node_id = BrokerId(-1);
                resp.port = -1;
            }
        }
        // v4 and later: one entry per key.
        for key in &req.coordinator_keys {
            let mut c = Coordinator::default();
            c.key = key.clone();
            c.error_message = Some(StrBytes::from_static_str(""));
            if let Some(error) = refused(key.as_str()) {
                c.error_code = error.code();
                c.node_id = BrokerId(-1);
                c.port = -1;
            } else if known {
                me(&mut c);
            } else {
                c.error_code = ResponseError::InvalidRequest.code();
                c.error_message =
                    invalid_message(req.key_type).or_else(|| Some(StrBytes::from_static_str("")));
                c.node_id = BrokerId(-1);
                c.port = -1;
            }
            resp.coordinators.push(c);
        }
        resp
    }
}

fn denied_words(error: ResponseError) -> &'static str {
    if error == ResponseError::GroupAuthorizationFailed {
        GROUP_DENIED
    } else {
        TXN_DENIED
    }
}

/// Kafka says nothing more for a share group's key type, and the usual words for any other.
fn invalid_message(key_type: i8) -> Option<StrBytes> {
    (key_type != 2).then(|| StrBytes::from_static_str(INVALID_REQUEST))
}
