//! The numbers on the wire for each enumeration of an ACL (Kafka 4.3.1: `ResourceType`, `PatternType`,
//! `AclOperation`, `AclPermissionType`). 0 is UNKNOWN in all four. ANY (and MATCH) are values of a filter, never
//! of an ACL that is stored.

/// Resource types.
pub mod resource {
    /// A filter's "every type".
    pub const ANY: i8 = 1;
    /// Topics.
    pub const TOPIC: i8 = 2;
    /// Consumer groups.
    pub const GROUP: i8 = 3;
    /// The cluster, which has one name.
    pub const CLUSTER: i8 = 4;
    /// Transactional ids.
    pub const TRANSACTIONAL_ID: i8 = 5;
    /// Delegation tokens (storable, never checked here).
    pub const DELEGATION_TOKEN: i8 = 6;
    /// Users (storable, never checked here).
    pub const USER: i8 = 7;

    /// Whether `code` is a resource type a request may name (anything else reads as UNKNOWN).
    #[must_use]
    pub const fn is_known(code: i8) -> bool {
        matches!(code, ANY..=USER)
    }
}

/// Pattern types.
pub mod pattern {
    /// A filter's "every pattern".
    pub const ANY: i8 = 1;
    /// A filter's "every ACL that applies to this name".
    pub const MATCH: i8 = 2;
    /// The resource named exactly (or, for the name `*`, every resource of the type).
    pub const LITERAL: i8 = 3;
    /// Every resource whose name starts with the ACL's.
    pub const PREFIXED: i8 = 4;

    /// Whether `code` is a pattern type a request may name.
    #[must_use]
    pub const fn is_known(code: i8) -> bool {
        matches!(code, ANY..=PREFIXED)
    }
}

/// Operations.
pub mod operation {
    /// A filter's "every operation".
    pub const ANY: i8 = 1;
    /// Every operation.
    pub const ALL: i8 = 2;
    /// Read.
    pub const READ: i8 = 3;
    /// Write.
    pub const WRITE: i8 = 4;
    /// Create.
    pub const CREATE: i8 = 5;
    /// Delete.
    pub const DELETE: i8 = 6;
    /// Alter.
    pub const ALTER: i8 = 7;
    /// Describe.
    pub const DESCRIBE: i8 = 8;
    /// Cluster action.
    pub const CLUSTER_ACTION: i8 = 9;
    /// Describe configs.
    pub const DESCRIBE_CONFIGS: i8 = 10;
    /// Alter configs.
    pub const ALTER_CONFIGS: i8 = 11;
    /// Idempotent write.
    pub const IDEMPOTENT_WRITE: i8 = 12;
    /// Create tokens.
    pub const CREATE_TOKENS: i8 = 13;
    /// Describe tokens.
    pub const DESCRIBE_TOKENS: i8 = 14;
    /// Two-phase commit.
    pub const TWO_PHASE_COMMIT: i8 = 15;

    /// Whether `code` is an operation a request may name.
    #[must_use]
    pub const fn is_known(code: i8) -> bool {
        matches!(code, ANY..=TWO_PHASE_COMMIT)
    }
}

/// Permission types.
pub mod permission {
    /// A filter's "either".
    pub const ANY: i8 = 1;
    /// Refuse.
    pub const DENY: i8 = 2;
    /// Permit.
    pub const ALLOW: i8 = 3;

    /// Whether `code` is a permission type a request may name.
    #[must_use]
    pub const fn is_known(code: i8) -> bool {
        matches!(code, ANY..=ALLOW)
    }
}
