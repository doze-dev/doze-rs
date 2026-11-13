//! What IAM holds: users, groups and roles (one record type, since they share almost everything), managed policies with their
//! versions, access keys, instance profiles, identity providers, and the account's own settings.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

/// Which kind of principal entity a record is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Kind {
    User,
    Group,
    Role,
}

impl Kind {
    /// `user`, `group`, `role`: the word in an ARN and in the record's key.
    pub(crate) const fn word(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Group => "group",
            Self::Role => "role",
        }
    }

    /// `User`, `Group`, `Role`: the word in names of members (`UserName`) and operations.
    pub(crate) const fn title(self) -> &'static str {
        match self {
            Self::User => "User",
            Self::Group => "Group",
            Self::Role => "Role",
        }
    }

    /// The prefix of the id IAM gives it.
    pub(crate) const fn id_prefix(self) -> &'static str {
        match self {
            Self::User => "AIDA",
            Self::Group => "AGPA",
            Self::Role => "AROA",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Entity {
    pub(crate) kind: Kind,
    pub(crate) name: String,
    pub(crate) path: String,
    pub(crate) id: String,
    pub(crate) arn: String,
    pub(crate) created_ms: i64,
    pub(crate) tags: BTreeMap<String, String>,
    /// Inline policies: name to document text.
    pub(crate) inline: BTreeMap<String, String>,
    /// ARNs of attached managed policies.
    pub(crate) attached: BTreeSet<String>,
    pub(crate) boundary: Option<String>,
    // A role's.
    pub(crate) trust: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) max_session_secs: i64,
    pub(crate) last_used: Option<(i64, String)>,
    pub(crate) service_linked: Option<String>,
    // A user's.
    pub(crate) groups: BTreeSet<String>,
    pub(crate) password_set_ms: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Version {
    pub(crate) id: String,
    pub(crate) document: String,
    pub(crate) created_ms: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Managed {
    pub(crate) arn: String,
    pub(crate) name: String,
    pub(crate) path: String,
    pub(crate) id: String,
    pub(crate) description: Option<String>,
    pub(crate) created_ms: i64,
    pub(crate) updated_ms: i64,
    /// Oldest first.
    pub(crate) versions: Vec<Version>,
    pub(crate) default: String,
    pub(crate) next_version: u32,
    pub(crate) tags: BTreeMap<String, String>,
    /// An AWS-managed policy (its ARN is `arn:aws:iam::aws:policy/...`), not the account's.
    pub(crate) aws_managed: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct AccessKey {
    pub(crate) id: String,
    pub(crate) secret: String,
    pub(crate) user: String,
    pub(crate) active: bool,
    pub(crate) created_ms: i64,
    pub(crate) last_used: Option<(i64, String, String)>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Profile {
    pub(crate) name: String,
    pub(crate) path: String,
    pub(crate) id: String,
    pub(crate) arn: String,
    pub(crate) created_ms: i64,
    pub(crate) roles: Vec<String>,
    pub(crate) tags: BTreeMap<String, String>,
}

/// An OpenID Connect or SAML provider: kept as given, and returned.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Provider {
    pub(crate) arn: String,
    pub(crate) saml: bool,
    pub(crate) created_ms: i64,
    /// OIDC: the URL, client ids and thumbprints. SAML: the metadata document.
    pub(crate) url: String,
    pub(crate) client_ids: Vec<String>,
    pub(crate) thumbprints: Vec<String>,
    pub(crate) metadata: String,
    pub(crate) tags: BTreeMap<String, String>,
}

/// The account itself: its aliases, its password policy, and the key that signs the sessions it hands out.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct Account {
    pub(crate) aliases: Vec<String>,
    /// The password policy as the request gave it (JSON), when one has been set.
    pub(crate) password_policy: Option<String>,
    pub(crate) signing_key: Vec<u8>,
}
