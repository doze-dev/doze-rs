//! What a principal may do: Apache Kafka 4.3.1's `StandardAuthorizer`, as pure functions over a set of ACLs.
//!
//! An [`Acl`] keeps the raw numbers a client sent, so a client reads back exactly what it stored. An [`AclSet`] is
//! the ACLs indexed by resource type for lookup. [`Rules`] is the decision: super users first, then the ACLs
//! that cover the resource (a DENY wins, then an ALLOW, with Kafka's implications), and the default only for a
//! resource nothing covers. [`AclFilter`] is what DescribeAcls and DeleteAcls match with.
//!
//! The rules and the Kafka source they come from are catalogued in `docs/port/behaviours/security.md`.

pub mod acl;
pub mod codes;
pub mod filter;
pub mod rules;
pub mod set;

mod by_type;

pub use acl::Acl;
pub use codes::{operation, pattern, permission, resource};
pub use filter::{AclFilter, FilterError};
pub use rules::{Resource, Rules, Subject, check_principal_shape};
pub use set::AclSet;

/// The one name the CLUSTER resource has.
pub const CLUSTER_NAME: &str = "kafka-cluster";

/// The wildcard of resource names (a LITERAL ACL named this covers every resource of its type) and of hosts.
pub const WILDCARD: &str = "*";

/// The principal that stands for every principal.
pub const WILDCARD_PRINCIPAL: &str = "User:*";

#[cfg(test)]
mod tests;
