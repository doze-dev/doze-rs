//! The view-models of the Access page (slice 9h) and its two dialogs, Create ACL and Delete ACL. Every name starts with
//! `Access` or `Acl` so that no other family of pages can meet it. Nothing here can hold a secret: a user is a name and
//! the mechanisms it may use and the iteration count of each SCRAM credential (a public parameter: the broker sends it to every
//! client in the server-first message), never a password, salt, stored key or server key.

use super::{Lines, Refusal, Span};

/// How an ACL's resource is named: the codes of Kafka's `ResourcePattern`.
#[derive(Debug, Clone, serde::Serialize, Copy, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum AclPattern {
    Literal,
    Prefixed,
}

/// Whether an ACL allows or denies.
#[derive(Debug, Clone, serde::Serialize, Copy, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum AclPermission {
    Allow,
    Deny,
}

/// One ACL as DescribeAcls says it: the wire numbers for the type and the operation, which are listed by
/// `acl_words`, and the rest as text.
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AclEntry {
    /// Kafka's resource type number (2 topic, 3 group, 4 cluster, 5 transactional id, 6 delegation token, 7 user).
    pub kind: i8,
    pub name: String,
    pub pattern: AclPattern,
    /// `User:orders`, or `User:*`.
    pub principal: String,
    /// `*` or an address.
    pub host: String,
    /// Kafka's operation number (3 read, 4 write, ...).
    pub operation: i8,
    pub permission: AclPermission,
}

/// Where a user came from.
#[derive(Debug, Clone, serde::Serialize, Copy, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum AccessFrom {
    /// The flags, a users file or the environment: re-applied at every start and not changeable over the API.
    Flags,
    /// Made over the API and kept in the metadata database.
    Stored,
}

/// One user, by name.
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AccessUser {
    pub name: String,
    pub from: AccessFrom,
    /// The mechanisms it has a credential for (SCRAM-SHA-256, ...), PLAIN where a flag user may use it.
    pub mechanisms: Vec<String>,
    /// The iteration count of each SCRAM credential, by mechanism, as DescribeUserScramCredentials says it. Empty for a user
    /// that has only PLAIN.
    pub iterations: Vec<(String, u32)>,
    /// Named in `--acl-super-user` as `User:name`.
    pub super_user: bool,
}

/// What each switch of Authentication says.
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AccessAuth {
    pub on: bool,
    pub mechanisms: Vec<String>,
    pub flag_users: usize,
    pub stored_users: usize,
    /// `waits 100 ms`, where the flags are known.
    pub failed_login: Option<String>,
    /// A PLAIN mechanism is on beside SCRAM ones, or not: the note under Mechanisms differs.
    pub plain: bool,
}

/// What the authorizer holds.
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AccessAuthz {
    pub default_allow: bool,
    pub acls: usize,
    pub allow: usize,
    pub deny: usize,
    pub super_users: Vec<String>,
}

/// Encryption of the Kafka listener.
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AccessTls {
    pub on: bool,
    /// The certificate's file, where the flags are known. The key's file and contents are never said.
    pub cert: Option<String>,
    pub listener: String,
    pub loopback: bool,
}

/// What the console is, and what protects it.
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AccessConsole {
    pub principal: String,
    pub listens: String,
    /// The console can be reached from other machines (`--console-unprotected` accepted it).
    pub beyond_loopback: bool,
    pub allow_hosts: Vec<String>,
}

/// One of the three start-up warnings and whether its condition holds now.
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AccessWarning {
    pub title: Vec<Span>,
    pub because: Vec<Span>,
    pub holds: bool,
}

/// The ACL table, or why it is not there.
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AccessAcls {
    pub rows: Vec<AclEntry>,
    /// ACLs the broker has beyond what the table lists.
    pub more: usize,
    /// DescribeAcls was refused (a caller that is not the console, on a broker with ACLs).
    #[serde(with = "crate::api::settings::refusal_opt")]
    #[cfg_attr(test, ts(as = "Option<crate::api::settings::KafkaRefusal>"))]
    pub refused: Option<Refusal>,
}

/// The page.
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AccessView {
    /// `127.0.0.1:9092`.
    pub listen: String,
    pub auth: AccessAuth,
    pub authz: AccessAuthz,
    pub tls: AccessTls,
    pub users: Vec<AccessUser>,
    pub acls: AccessAcls,
    pub console: AccessConsole,
    pub warnings: Vec<AccessWarning>,
}

/// What the Create ACL form carries, as words (anything else reads as the first of its kind).
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AclInput {
    pub kind: String,
    pub name: String,
    pub pattern: String,
    pub principal: String,
    pub host: String,
    pub operation: String,
    pub permission: String,
}

/// The Create ACL dialog, with what was typed, and what Kafka said if it said no.
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AclCreateView {
    pub input: AclInput,
    /// A field that is missing, by name and in words.
    pub error: Option<(String, String)>,
    #[serde(with = "crate::api::settings::refusal_opt")]
    #[cfg_attr(test, ts(as = "Option<crate::api::settings::KafkaRefusal>"))]
    pub refused: Option<Refusal>,
    pub call: Lines,
    /// The words of the kind, for the teaching line (`topic`).
    pub kind_word: String,
}

/// The Delete ACL dialog.
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AclDeleteView {
    pub entry: AclEntry,
    /// What the broker would decide for this principal afterwards, by Kafka's own rules.
    pub after: Vec<Span>,
    pub call: Lines,
    /// The ACL is no longer there (a second tab, a tool): nothing to delete.
    pub gone: bool,
    #[serde(with = "crate::api::settings::refusal_opt")]
    #[cfg_attr(test, ts(as = "Option<crate::api::settings::KafkaRefusal>"))]
    pub refused: Option<Refusal>,
}

/// What a Create ACL submit came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AclCreateResult {
    Created,
    Invalid(Box<AclCreateView>),
    Refused(Box<AclCreateView>),
}

/// What a Delete ACL submit came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AclDeleteResult {
    Deleted,
    Gone(Box<AclDeleteView>),
    Refused(Box<AclDeleteView>),
}
