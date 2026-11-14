//! The numbers on the wire for each enumeration of an ACL (Kafka 4.3.1: `ResourceType`, `PatternType`,
//! `AclOperation`, `AclPermissionType`). 0 is UNKNOWN in all four. ANY (and MATCH) are values of a filter, never
//! of an ACL that is stored.
//!
//! An ACL keeps the numbers a client sent (it reads back what it stored), so the modules below are those numbers. What
//! is *asked* of the authorizer is typed: a [`ResourceType`] and an [`Operation`], read from a number with `TryFrom` at
//! the wire's edge.

/// A number that names no value of the enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("{0} is not a known code")]
pub struct UnknownCode(pub i8);

macro_rules! wire_enum {
    ($(#[$m:meta])* $name:ident { $($(#[$vm:meta])* $variant:ident = $code:literal,)* }) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        #[repr(i8)]
        pub enum $name {
            $($(#[$vm])* $variant = $code,)*
        }

        impl $name {
            /// The number on the wire.
            #[must_use]
            pub const fn code(self) -> i8 {
                self as i8
            }
        }

        impl TryFrom<i8> for $name {
            type Error = UnknownCode;

            fn try_from(code: i8) -> Result<Self, UnknownCode> {
                match code {
                    $($code => Ok(Self::$variant),)*
                    other => Err(UnknownCode(other)),
                }
            }
        }
    };
}

wire_enum! {
    /// A resource type a request may name.
    ResourceType {
        /// A filter's "every type".
        Any = 1,
        /// Topics.
        Topic = 2,
        /// Consumer groups.
        Group = 3,
        /// The cluster, which has one name.
        Cluster = 4,
        /// Transactional ids.
        TransactionalId = 5,
        /// Delegation tokens (storable, never checked here).
        DelegationToken = 6,
        /// Users (storable, never checked here).
        User = 7,
    }
}

wire_enum! {
    /// An operation a request may name.
    Operation {
        /// A filter's "every operation".
        Any = 1,
        /// Every operation.
        All = 2,
        Read = 3,
        Write = 4,
        Create = 5,
        Delete = 6,
        Alter = 7,
        Describe = 8,
        ClusterAction = 9,
        DescribeConfigs = 10,
        AlterConfigs = 11,
        IdempotentWrite = 12,
        CreateTokens = 13,
        DescribeTokens = 14,
        TwoPhaseCommit = 15,
    }
}

/// Resource types.
pub mod resource {
    use super::ResourceType;

    /// A filter's "every type".
    pub const ANY: i8 = ResourceType::Any.code();
    /// Topics.
    pub const TOPIC: i8 = ResourceType::Topic.code();
    /// Consumer groups.
    pub const GROUP: i8 = ResourceType::Group.code();
    /// The cluster, which has one name.
    pub const CLUSTER: i8 = ResourceType::Cluster.code();
    /// Transactional ids.
    pub const TRANSACTIONAL_ID: i8 = ResourceType::TransactionalId.code();
    /// Delegation tokens (storable, never checked here).
    pub const DELEGATION_TOKEN: i8 = ResourceType::DelegationToken.code();
    /// Users (storable, never checked here).
    pub const USER: i8 = ResourceType::User.code();

    /// Whether `code` is a resource type a request may name (anything else reads as UNKNOWN).
    #[must_use]
    pub fn is_known(code: i8) -> bool {
        ResourceType::try_from(code).is_ok()
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
    use super::Operation;

    /// A filter's "every operation".
    pub const ANY: i8 = Operation::Any.code();
    /// Every operation.
    pub const ALL: i8 = Operation::All.code();
    /// Read.
    pub const READ: i8 = Operation::Read.code();
    /// Write.
    pub const WRITE: i8 = Operation::Write.code();
    /// Create.
    pub const CREATE: i8 = Operation::Create.code();
    /// Delete.
    pub const DELETE: i8 = Operation::Delete.code();
    /// Alter.
    pub const ALTER: i8 = Operation::Alter.code();
    /// Describe.
    pub const DESCRIBE: i8 = Operation::Describe.code();
    /// Cluster action.
    pub const CLUSTER_ACTION: i8 = Operation::ClusterAction.code();
    /// Describe configs.
    pub const DESCRIBE_CONFIGS: i8 = Operation::DescribeConfigs.code();
    /// Alter configs.
    pub const ALTER_CONFIGS: i8 = Operation::AlterConfigs.code();
    /// Idempotent write.
    pub const IDEMPOTENT_WRITE: i8 = Operation::IdempotentWrite.code();
    /// Create tokens.
    pub const CREATE_TOKENS: i8 = Operation::CreateTokens.code();
    /// Describe tokens.
    pub const DESCRIBE_TOKENS: i8 = Operation::DescribeTokens.code();
    /// Two-phase commit.
    pub const TWO_PHASE_COMMIT: i8 = Operation::TwoPhaseCommit.code();

    /// Whether `code` is an operation a request may name.
    #[must_use]
    pub fn is_known(code: i8) -> bool {
        Operation::try_from(code).is_ok()
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
