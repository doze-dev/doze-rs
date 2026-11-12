//! The checks the group and transaction APIs make, asked of the authorizer in one place.
//!
//! Kafka makes them in the request handler, before the group coordinator or the transaction coordinator is
//! asked anything (`KafkaApis.scala`); here too they are made in the handler, outside the group's actor, once
//! per request (once per group, topic or id the request names). With nothing stored and the default allowing,
//! each is the authorizer's one atomic load.

use kafka_auth::authz::{CLUSTER_NAME, operation, resource};
use kafka_storage::fs::Fs;

use crate::authorizer::Authorizer;
use crate::{Broker, Caller};

/// What a group can be asked to allow (Kafka's `AclEntry.supportedOperations(GROUP)`): the bits of an
/// authorized-operations field.
const GROUP_OPERATIONS: [i8; 5] = [
    operation::READ,
    operation::DELETE,
    operation::DESCRIBE,
    operation::DESCRIBE_CONFIGS,
    operation::ALTER_CONFIGS,
];

/// One caller's rights, for the length of one request.
pub(crate) struct Guard<'a> {
    authz: &'a Authorizer,
    who: &'a Caller,
}

impl Guard<'_> {
    /// May the caller do `op` to the group `id`?
    pub(crate) fn group(&self, op: i8, id: &str) -> bool {
        self.authz.allows(self.who, resource::GROUP, id, op)
    }

    /// May the caller do `op` to the topic `name`?
    pub(crate) fn topic(&self, op: i8, name: &str) -> bool {
        self.authz.allows(self.who, resource::TOPIC, name, op)
    }

    /// May the caller do `op` to the transactional id `id`?
    pub(crate) fn txn(&self, op: i8, id: &str) -> bool {
        self.authz
            .allows(self.who, resource::TRANSACTIONAL_ID, id, op)
    }

    /// May the caller do `op` to the cluster?
    pub(crate) fn cluster(&self, op: i8) -> bool {
        self.authz
            .allows(self.who, resource::CLUSTER, CLUSTER_NAME, op)
    }

    /// The operations the caller may do to the group `id`, one bit each at the operation's number
    /// (`AuthHelper.authorizedOperations`: ALL is never a bit).
    pub(crate) fn group_operations(&self, id: &str) -> i32 {
        GROUP_OPERATIONS
            .iter()
            .filter(|op| self.group(**op, id))
            .fold(0_i32, |bits, op| bits | (1_i32 << *op))
    }
}

impl<F: Fs> Broker<F> {
    /// The rights of `who` to check a request against.
    pub(crate) fn guard<'a>(&'a self, who: &'a Caller) -> Guard<'a> {
        Guard {
            authz: &self.authz,
            who,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bits_of_a_group_are_the_operation_numbers_and_never_all() {
        let bits = GROUP_OPERATIONS
            .iter()
            .fold(0_i32, |b, op| b | (1_i32 << *op));
        // READ 3, DELETE 6, DESCRIBE 8, DESCRIBE_CONFIGS 10, ALTER_CONFIGS 11.
        assert_eq!(bits, 8 + 64 + 256 + 1024 + 2048);
    }
}
