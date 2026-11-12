//! The checks of the topic APIs: may this caller do an operation on a topic, or on the cluster. Every handler asks
//! here, so that the resource type, the cluster's name and the denial texts are written once.
//!
//! With nothing stored and the default allowing, each check is one atomic load (`Authorizer::allows`) and
//! allocates nothing, which is why a handler may ask once per topic of a request, on the Produce and Fetch paths.

use kafka_auth::authz::{CLUSTER_NAME, operation, resource};
use kafka_storage::fs::Fs;

use crate::{Broker, Caller};

/// What Kafka says of a denied topic where the error has a text of its own (`Errors.TOPIC_AUTHORIZATION_FAILED`).
pub(crate) const TOPIC_DENIED: &str = "Topic authorization failed.";
/// `Errors.CLUSTER_AUTHORIZATION_FAILED`.
pub(crate) const CLUSTER_DENIED: &str = "Cluster authorization failed.";
/// `Errors.GROUP_AUTHORIZATION_FAILED`.
pub(crate) const GROUP_DENIED: &str = "Group authorization failed.";

/// The operations an ACL on a topic can name, which are the bits of a topic's authorized-operations field
/// (`AclEntry.supportedOperations(TOPIC)`).
const TOPIC_OPERATIONS: [i8; 8] = [
    operation::READ,
    operation::WRITE,
    operation::CREATE,
    operation::DESCRIBE,
    operation::DELETE,
    operation::ALTER,
    operation::DESCRIBE_CONFIGS,
    operation::ALTER_CONFIGS,
];

/// The same for the cluster resource (`AclEntry.supportedOperations(CLUSTER)`).
const CLUSTER_OPERATIONS: [i8; 7] = [
    operation::CREATE,
    operation::CLUSTER_ACTION,
    operation::DESCRIBE_CONFIGS,
    operation::ALTER_CONFIGS,
    operation::IDEMPOTENT_WRITE,
    operation::ALTER,
    operation::DESCRIBE,
];

impl<F: Fs> Broker<F> {
    /// Whether `who` may do `op` on the topic called `topic` (the name as the client sent it, whether the topic exists
    /// or not: Kafka asks before it looks).
    pub(crate) fn topic_allows(&self, who: &Caller, topic: &str, op: i8) -> bool {
        self.authz.allows(who, resource::TOPIC, topic, op)
    }

    /// Whether `who` may do `op` on the cluster.
    pub(crate) fn cluster_allows(&self, who: &Caller, op: i8) -> bool {
        self.authz.allows(who, resource::CLUSTER, CLUSTER_NAME, op)
    }

    /// The topic's authorized operations as Kafka reports them: bit `1 << code` for each operation a topic ACL can
    /// name that `who` may do (`AuthHelper.authorizedOperations`; ALL is never a bit).
    pub(crate) fn topic_operations(&self, who: &Caller, topic: &str) -> i32 {
        bits(&TOPIC_OPERATIONS, |op| self.topic_allows(who, topic, op))
    }

    /// The cluster's authorized operations: 0 if `who` may not even DESCRIBE the cluster.
    pub(crate) fn cluster_operations(&self, who: &Caller) -> i32 {
        if !self.cluster_allows(who, operation::DESCRIBE) {
            return 0;
        }
        bits(&CLUSTER_OPERATIONS, |op| self.cluster_allows(who, op))
    }
}

fn bits(ops: &[i8], allowed: impl Fn(i8) -> bool) -> i32 {
    ops.iter().filter(|op| allowed(**op)).fold(0, |field, op| {
        field | 1_i32.checked_shl(u32::from(op.unsigned_abs())).unwrap_or(0)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fields_are_utils_to_32_bit_field_of_the_operation_codes() {
        // READ 3, WRITE 4, CREATE 5, DELETE 6, ALTER 7, DESCRIBE 8, DESCRIBE_CONFIGS 10, ALTER_CONFIGS 11.
        let every = TOPIC_OPERATIONS.iter().fold(0_i32, |f, op| f | (1 << op));
        assert_eq!(every, 0b1101_1111_1000);
        assert_eq!(bits(&TOPIC_OPERATIONS, |_| true), every);
        assert_eq!(bits(&TOPIC_OPERATIONS, |_| false), 0);
        assert_eq!(
            bits(&CLUSTER_OPERATIONS, |_| true),
            (1 << 5) | (1 << 9) | (1 << 10) | (1 << 11) | (1 << 12) | (1 << 7) | (1 << 8)
        );
    }
}
