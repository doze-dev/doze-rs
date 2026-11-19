//! FindCoordinator: this broker, for every group and every transaction.

use kafka_auth::authz::Operation;
use kafka_storage::fs::Fs;
use kafka_wire::ResponseError;
use kafka_wire::StrBytes;
use kafka_wire::messages::find_coordinator_response::Coordinator;
use kafka_wire::messages::{BrokerId, FindCoordinatorRequest, FindCoordinatorResponse};

use crate::{Broker, Caller, Listener};

/// Kafka's words for a request it cannot make sense of.
const INVALID_REQUEST: &str = "This most likely occurs because of a request being malformed by the client library or the message was sent to an incompatible broker. See the broker logs for more details.";

/// A group's coordinator is a key type 0, a transaction's 1, a share partition's state 2 (from v6, KIP-932).
const GROUP: i8 = 0;
const TRANSACTION: i8 = 1;
const SHARE: i8 = 2;

/// What Kafka says for a whole request refused CLUSTER_ACTION (`Errors.CLUSTER_AUTHORIZATION_FAILED.message()`).
const CLUSTER_DENIED: &str = "Cluster authorization failed.";

/// The words of a refusal before v4, which Kafka takes from the error itself (`prepareOldResponse`).
const GROUP_DENIED: &str = "Group authorization failed.";
const TXN_DENIED: &str = "Transactional Id authorization failed.";

impl<F: Fs> Broker<F> {
    /// A single node is its own coordinator. Versions 0 to 3 ask for one key and v4 and later for a list; both are
    /// answered here, the answer to a key type that is no coordinator's being INVALID_REQUEST with no broker.
    ///
    /// Before any of that, the caller must be allowed to DESCRIBE the group (key type 0) or the transactional id
    /// (key type 1): a refusal is 30 or 53, with no broker, for the key (v4 and later) or at the top (before).
    ///
    /// A share partition's state (key type 2, `group:topicId:partition`) is asked for by brokers, from v6 (v5 and below
    /// answer it INVALID_REQUEST): the caller needs CLUSTER_ACTION, else every key of the request is 31 (Kafka throws for
    /// the request); a key not of that form is INVALID_REQUEST; any other is this broker, which keeps share groups'
    /// state itself (`KafkaApis.getCoordinator`).
    pub(crate) fn find_coordinator(
        &self,
        version: i16,
        at: &Listener,
        caller: &Caller,
        req: &FindCoordinatorRequest,
    ) -> FindCoordinatorResponse {
        let guard = self.guard(caller);
        let refused = |key: &str| match req.key_type {
            GROUP if !guard.group(Operation::Describe, key) => {
                Some(ResponseError::GroupAuthorizationFailed)
            }
            TRANSACTION if !guard.txn(Operation::Describe, key) => {
                Some(ResponseError::TransactionalIdAuthorizationFailed)
            }
            _ => None,
        };
        let known = matches!(req.key_type, GROUP | TRANSACTION) || version == 0;
        let share = req.key_type == SHARE && version >= 6;
        let share_groups_run =
            kafka_wire::profile::Profile::new(self.config.kafka_major).share_version() > 0;
        if share && !guard.cluster(Operation::ClusterAction) {
            return cluster_refused(req);
        }
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
                // (Kafka 1.1 leaves it null.)
                if (1..=3).contains(&version) {
                    resp.error_message = (self.config.kafka_major.get() >= 2)
                        .then(|| StrBytes::from_static_str("NONE"));
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
            } else if share && !share_groups_run {
                // Kafka 3.9 lists v6 with share groups off: no share coordinator, whatever the key.
                c.error_code = ResponseError::CoordinatorNotAvailable.code();
                c.node_id = BrokerId(-1);
                c.port = -1;
            } else if known || (share && share_key(key.as_str())) {
                me(&mut c);
            } else if share {
                c.error_code = ResponseError::InvalidRequest.code();
                c.node_id = BrokerId(-1);
                c.port = -1;
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

/// Every key of a v6 request for share state, refused CLUSTER_ACTION: `FindCoordinatorResponse.prepareErrorResponse`.
fn cluster_refused(req: &FindCoordinatorRequest) -> FindCoordinatorResponse {
    let mut resp = FindCoordinatorResponse::default();
    for key in &req.coordinator_keys {
        let mut c = Coordinator::default();
        c.key = key.clone();
        c.error_code = ResponseError::ClusterAuthorizationFailed.code();
        c.error_message = Some(StrBytes::from_static_str(CLUSTER_DENIED));
        c.node_id = BrokerId(-1);
        c.port = -1;
        resp.coordinators.push(c);
    }
    resp
}

/// Whether a share coordinator key reads as Kafka's `SharePartitionKey.getInstance` reads it: split at the colons (Java's
/// `split`, which drops empty pieces at the end), the last piece a partition number, the one before a topic id, and the
/// rest, joined again, a group id that is not blank.
fn share_key(key: &str) -> bool {
    let mut pieces: Vec<&str> = key.split(':').collect();
    while pieces.last().is_some_and(|p| p.is_empty()) {
        pieces.pop();
    }
    let [group @ .., topic, partition] = pieces.as_slice() else {
        return false;
    };
    !group.is_empty()
        && !group.join(":").trim().is_empty()
        && topic_id(topic)
        && java_int(partition)
}

/// `Uuid.fromString`: URL-safe base64 of 16 bytes, which is 22 characters, or 24 with the padding.
fn topic_id(s: &str) -> bool {
    let body = s.strip_suffix("==").unwrap_or(s);
    body.len() == 22
        && body
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// `Integer.parseInt`: an optional sign, then ASCII digits, inside an `i32`.
fn java_int(s: &str) -> bool {
    let digits = s.strip_prefix(['-', '+']).unwrap_or(s);
    !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) && s.parse::<i32>().is_ok()
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
