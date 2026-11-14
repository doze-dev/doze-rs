//! The words of ACLs: Kafka's names for the numbers a client sends, the codes a form carries, and what deleting one comes
//! to, decided by the broker's own rules ([`kafka_auth::authz::Rules`]) over the ACLs that would be left.
//!
//! Pure functions with tables of cases. Nothing here asks the broker anything.

use kafka_auth::authz::{Acl, AclSet, Resource, Rules, Subject, WILDCARD, operation, resource};
use kafka_wire::ResponseError;

use crate::model::{AclEntry, AclPattern, AclPermission, Span};

/// A resource type a form may name: the word it carries, the label the table shows, the name on the wire as the boards
/// draw the call, and the number.
pub const KINDS: [(&str, &str, &str, i8); 6] = [
    ("topic", "Topic", "TOPIC", resource::TOPIC),
    ("group", "Group", "GROUP", resource::GROUP),
    ("cluster", "Cluster", "CLUSTER", resource::CLUSTER),
    (
        "transactional-id",
        "Transactional id",
        "TRANSACTIONAL_ID",
        resource::TRANSACTIONAL_ID,
    ),
    (
        "delegation-token",
        "Delegation token",
        "DELEGATION_TOKEN",
        resource::DELEGATION_TOKEN,
    ),
    ("user", "User", "USER", resource::USER),
];

/// An operation a form may name: the word it carries, the label (Kafka's CamelCase), the name on the wire, the number.
pub const OPERATIONS: [(&str, &str, &str, i8); 14] = [
    ("all", "All", "ALL", operation::ALL),
    ("read", "Read", "READ", operation::READ),
    ("write", "Write", "WRITE", operation::WRITE),
    ("create", "Create", "CREATE", operation::CREATE),
    ("delete", "Delete", "DELETE", operation::DELETE),
    ("alter", "Alter", "ALTER", operation::ALTER),
    ("describe", "Describe", "DESCRIBE", operation::DESCRIBE),
    (
        "cluster-action",
        "ClusterAction",
        "CLUSTER_ACTION",
        operation::CLUSTER_ACTION,
    ),
    (
        "describe-configs",
        "DescribeConfigs",
        "DESCRIBE_CONFIGS",
        operation::DESCRIBE_CONFIGS,
    ),
    (
        "alter-configs",
        "AlterConfigs",
        "ALTER_CONFIGS",
        operation::ALTER_CONFIGS,
    ),
    (
        "idempotent-write",
        "IdempotentWrite",
        "IDEMPOTENT_WRITE",
        operation::IDEMPOTENT_WRITE,
    ),
    (
        "create-tokens",
        "CreateTokens",
        "CREATE_TOKENS",
        operation::CREATE_TOKENS,
    ),
    (
        "describe-tokens",
        "DescribeTokens",
        "DESCRIBE_TOKENS",
        operation::DESCRIBE_TOKENS,
    ),
    (
        "two-phase-commit",
        "TwoPhaseCommit",
        "TWO_PHASE_COMMIT",
        operation::TWO_PHASE_COMMIT,
    ),
];

/// The label of a resource type (`Topic`), or `Type 9` for a number that is none.
#[must_use]
pub fn kind_label(kind: i8) -> String {
    KINDS
        .iter()
        .find(|k| k.3 == kind)
        .map_or_else(|| format!("Type {kind}"), |k| k.1.to_owned())
}

/// The name of a resource type on the wire (`TRANSACTIONAL_ID`).
#[must_use]
pub fn kind_wire(kind: i8) -> String {
    KINDS
        .iter()
        .find(|k| k.3 == kind)
        .map_or_else(|| format!("UNKNOWN_{kind}"), |k| k.2.to_owned())
}

/// The resource type a form's word names.
#[must_use]
pub fn kind_from_form(word: &str) -> Option<i8> {
    KINDS.iter().find(|k| k.0 == word).map(|k| k.3)
}

/// The word a form carries for a resource type.
#[must_use]
pub fn kind_form(kind: i8) -> &'static str {
    KINDS.iter().find(|k| k.3 == kind).map_or("topic", |k| k.0)
}

/// The label of an operation (`IdempotentWrite`), or `Operation 9` for a number that is none.
#[must_use]
pub fn operation_label(op: i8) -> String {
    OPERATIONS
        .iter()
        .find(|o| o.3 == op)
        .map_or_else(|| format!("Operation {op}"), |o| o.1.to_owned())
}

/// The name of an operation on the wire (`IDEMPOTENT_WRITE`).
#[must_use]
pub fn operation_wire(op: i8) -> String {
    OPERATIONS
        .iter()
        .find(|o| o.3 == op)
        .map_or_else(|| format!("UNKNOWN_{op}"), |o| o.2.to_owned())
}

/// The operation a form's word names.
#[must_use]
pub fn operation_from_form(word: &str) -> Option<i8> {
    OPERATIONS.iter().find(|o| o.0 == word).map(|o| o.3)
}

/// The word a form carries for an operation.
#[must_use]
pub fn operation_form(op: i8) -> &'static str {
    OPERATIONS
        .iter()
        .find(|o| o.3 == op)
        .map_or("read", |o| o.0)
}

impl AclPattern {
    /// The label the table shows.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Literal => "Literal",
            Self::Prefixed => "Prefixed",
        }
    }

    /// The name on the wire.
    #[must_use]
    pub const fn wire(self) -> &'static str {
        match self {
            Self::Literal => "LITERAL",
            Self::Prefixed => "PREFIXED",
        }
    }

    /// The word a form carries.
    #[must_use]
    pub const fn form(self) -> &'static str {
        match self {
            Self::Literal => "literal",
            Self::Prefixed => "prefixed",
        }
    }

    /// The pattern a form's word names; anything else is literal.
    #[must_use]
    pub fn from_form(word: &str) -> Self {
        if word == "prefixed" {
            Self::Prefixed
        } else {
            Self::Literal
        }
    }
}

impl AclPermission {
    /// The label the table shows.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Allow => "Allow",
            Self::Deny => "Deny",
        }
    }

    /// The name on the wire.
    #[must_use]
    pub const fn wire(self) -> &'static str {
        match self {
            Self::Allow => "ALLOW",
            Self::Deny => "DENY",
        }
    }

    /// The word a form carries.
    #[must_use]
    pub const fn form(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Deny => "deny",
        }
    }

    /// The permission a form's word names; anything else is allow.
    #[must_use]
    pub fn from_form(word: &str) -> Self {
        if word == "deny" {
            Self::Deny
        } else {
            Self::Allow
        }
    }
}

impl AclEntry {
    /// The entry as the broker's own type, for its rules.
    #[must_use]
    pub fn to_acl(&self) -> Acl {
        Acl {
            resource_type: self.kind,
            name: self.name.clone(),
            pattern_type: match self.pattern {
                AclPattern::Literal => kafka_auth::authz::pattern::LITERAL,
                AclPattern::Prefixed => kafka_auth::authz::pattern::PREFIXED,
            },
            principal: self.principal.clone(),
            host: self.host.clone(),
            operation: self.operation,
            permission: match self.permission {
                AclPermission::Allow => kafka_auth::authz::permission::ALLOW,
                AclPermission::Deny => kafka_auth::authz::permission::DENY,
            },
        }
    }

    /// The query a link carries to the delete dialog of this exact ACL.
    #[must_use]
    pub fn query(&self) -> String {
        let q = crate::form::encode_query(&[
            ("type", kind_form(self.kind)),
            ("name", &self.name),
            ("pattern", self.pattern.form()),
            ("principal", &self.principal),
            ("host", &self.host),
            ("op", operation_form(self.operation)),
            ("perm", self.permission.form()),
        ]);
        q.trim_start_matches('?').to_owned()
    }
}

/// The API and the error a refusal of `op` on a resource of type `kind` is met as: what a client's next request of that
/// kind is answered. `None` for a pair the pages do not name.
#[must_use]
pub fn refused_as(kind: i8, op: i8) -> (Option<&'static str>, ResponseError) {
    let error = match kind {
        resource::GROUP => ResponseError::GroupAuthorizationFailed,
        resource::CLUSTER => ResponseError::ClusterAuthorizationFailed,
        resource::TRANSACTIONAL_ID => ResponseError::TransactionalIdAuthorizationFailed,
        _ => ResponseError::TopicAuthorizationFailed,
    };
    let api = match (kind, op) {
        (resource::TOPIC, operation::WRITE) => Some("Produce"),
        (resource::TOPIC, operation::READ) => Some("Fetch"),
        (resource::TOPIC, operation::DESCRIBE) => Some("Metadata"),
        (resource::TOPIC, operation::CREATE) => Some("CreateTopics"),
        (resource::TOPIC, operation::DELETE) => Some("DeleteTopics"),
        (resource::TOPIC, operation::ALTER) => Some("CreatePartitions"),
        (resource::GROUP, operation::READ) => Some("JoinGroup"),
        (resource::GROUP, operation::DESCRIBE) => Some("DescribeGroups"),
        (resource::GROUP, operation::DELETE) => Some("DeleteGroups"),
        (resource::TRANSACTIONAL_ID, operation::WRITE) => Some("InitProducerId"),
        (resource::CLUSTER, operation::IDEMPOTENT_WRITE) => Some("InitProducerId"),
        _ => None,
    };
    (api, error)
}

/// The operations a deleted ACL is judged by: an `All` is judged by the two that matter most.
fn judged(op: i8) -> Vec<i8> {
    if op == operation::ALL {
        vec![operation::READ, operation::WRITE]
    } else {
        vec![op]
    }
}

fn decide(rules: &Rules, set: &AclSet, e: &AclEntry, op: i8) -> bool {
    let host = if e.host == WILDCARD {
        "127.0.0.1"
    } else {
        e.host.as_str()
    };
    rules.authorize(
        set,
        Subject {
            principal: &e.principal,
            host,
        },
        Resource {
            kind: e.kind,
            name: &e.name,
        },
        op,
    )
}

/// What the broker would decide for the ACL's own principal, resource and operation once the ACL is gone, in words.
/// `all` is every ACL the broker has now (the one to delete included).
#[must_use]
pub fn after_delete(all: &[AclEntry], target: &AclEntry, rules: &Rules) -> Vec<Span> {
    let before = AclSet::new(all.iter().map(AclEntry::to_acl));
    let gone = target.to_acl();
    let left: Vec<Acl> = before.iter().filter(|a| **a != gone).cloned().collect();
    let after = AclSet::new(left.iter().cloned());
    let who = target.principal.as_str();
    let res = format!("{} {}", kind_label(target.kind).to_lowercase(), target.name);
    if rules.super_users.contains(who) {
        return vec![
            Span::mono(who),
            Span::text(" is a super user: no ACL refuses it, so nothing changes for it."),
        ];
    }
    let covered = left
        .iter()
        .filter(|a| a.resource_type == target.kind && a.covers(&target.name))
        .count();
    let mut out = vec![Span::mono(target.name.clone())];
    out.push(Span::text(if covered == 0 {
        " is then covered by no ACL, so the default decides: ".to_owned()
    } else {
        format!(
            " is also covered by {covered} other ACL{}, so the default stays off for it and ",
            if covered == 1 { "" } else { "s" }
        )
    }));
    let verdicts: Vec<(i8, bool, bool)> = judged(target.operation)
        .into_iter()
        .map(|op| {
            (
                op,
                decide(rules, &before, target, op),
                decide(rules, &after, target, op),
            )
        })
        .collect();
    out.extend(outcome(who, &res, target.kind, &verdicts, covered > 0));
    out
}

fn outcome(who: &str, res: &str, kind: i8, v: &[(i8, bool, bool)], covered: bool) -> Vec<Span> {
    let lost = v.iter().find(|(_, b, a)| *b && !*a);
    let gained = v.iter().find(|(_, b, a)| !*b && *a);
    if let Some(&(op, _, _)) = lost {
        let (api, err) = refused_as(kind, op);
        let what = api.map_or_else(|| "next request".to_owned(), |a| format!("next {a}"));
        return vec![
            Span::mono(who),
            Span::text(format!(" is then refused: its {what} is answered ")),
            Span::mono(crate::refusal::code_name(err.code())),
            Span::text(format!(" ({}).", err.code())),
        ];
    }
    if let Some(&(op, _, _)) = gained {
        return vec![
            Span::mono(who),
            Span::text(format!(
                " is then allowed {} on {res}.",
                operation_label(op)
            )),
        ];
    }
    let still_allowed = v.iter().any(|(_, _, a)| *a);
    let why = if still_allowed && covered {
        "still allowed by another ACL."
    } else if still_allowed {
        "still allowed by the default (--acl-default allow)."
    } else {
        "refused, as it was."
    };
    vec![Span::mono(who), Span::text(format!(" is {why}"))]
}

#[cfg(test)]
mod tests;
