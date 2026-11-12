//! The decision: `StandardAuthorizerData.authorize` and `findResult` of Kafka 4.3.1.

use std::collections::BTreeSet;

use super::set::ByType;
use super::{Acl, AclSet, WILDCARD, WILDCARD_PRINCIPAL, operation, permission};

/// Who is asking, and from where: the principal as `Type:name`, and the textual address of the client.
#[derive(Debug, Clone, Copy)]
pub struct Subject<'a> {
    /// The connection's principal.
    pub principal: &'a str,
    /// The client's address as Java's `getHostAddress` writes it.
    pub host: &'a str,
}

/// What is asked about: a resource of a type, by its name (always a concrete name, never a pattern).
#[derive(Debug, Clone, Copy)]
pub struct Resource<'a> {
    /// The resource type (`codes::resource`).
    pub kind: i8,
    /// The name.
    pub name: &'a str,
}

/// What the ACLs say about one request, before the default is applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Verdict {
    Allow,
    Deny,
    /// No ACL covers the resource, whoever it is for: the default decides.
    Uncovered,
}

/// The settings of the authorizer: who is never refused, and what a resource that no ACL covers allows.
#[derive(Debug, Clone, Default)]
pub struct Rules {
    /// `allow.everyone.if.no.acl.found`.
    pub default_allow: bool,
    /// `super.users`, as `Type:name`.
    pub super_users: BTreeSet<String>,
}

/// Whether `text` is written `Type:name`, with neither part empty (Kafka only asks for a colon; an empty type or
/// name can never be a principal anyone logs in as).
#[must_use]
pub fn check_principal_shape(text: &str) -> bool {
    text.split_once(':')
        .is_some_and(|(kind, name)| !kind.is_empty() && !name.is_empty())
}

impl Rules {
    /// Whether `who` may do `op` on `resource`. A super user may do anything, DENY ACLs notwithstanding; for the
    /// rest a covering DENY refuses, else a covering ALLOW permits, else the default applies if and only if no
    /// ACL covers the resource (an ACL for anybody else turns the default off for it).
    #[must_use]
    pub fn authorize(
        &self,
        set: &AclSet,
        who: Subject<'_>,
        resource: Resource<'_>,
        op: i8,
    ) -> bool {
        if self.super_users.contains(who.principal) {
            return true;
        }
        match verdict(set, who, resource, op) {
            Verdict::Allow => true,
            Verdict::Deny => false,
            Verdict::Uncovered => self.default_allow,
        }
    }
}

/// What the ACLs that cover `resource` decide for `who` doing `op`.
pub(super) fn verdict(set: &AclSet, who: Subject<'_>, resource: Resource<'_>, op: i8) -> Verdict {
    let Some(by_type) = set.of_type(resource.kind) else {
        return Verdict::Uncovered;
    };
    let (mut covered, mut allowed) = (false, false);
    for acl in covering(by_type, resource.name) {
        covered = true;
        match result_of(acl, who, op) {
            Some(true) => allowed = true,
            Some(false) => return Verdict::Deny,
            None => {}
        }
    }
    match (allowed, covered) {
        (true, _) => Verdict::Allow,
        (false, true) => Verdict::Deny,
        (false, false) => Verdict::Uncovered,
    }
}

/// The ACLs of the type whose resource pattern covers `name`: those named so exactly, the `*` ones, and the
/// prefixes of it. Whether they are for this principal, host or operation is not asked.
///
/// Kafka's walk skips a LITERAL ACL whose name is only a prefix of the resource's, and that includes the LITERAL
/// `*` when the resource itself starts with an asterisk (a group or transactional id called `*x`): the wildcard
/// does not cover it (SAD:398-405, 354-362). That is kept, for parity.
fn covering<'a>(by_type: &'a ByType, name: &'a str) -> impl Iterator<Item = &'a Acl> {
    let exact = by_type.literal.get(name).into_iter().flatten();
    let wildcard = by_type
        .literal
        .get(WILDCARD)
        .into_iter()
        .flatten()
        .filter(move |_| !name.starts_with(WILDCARD));
    let prefixed = by_type.prefixed.iter().filter(move |a| a.covers(name));
    exact.chain(wildcard).chain(prefixed)
}

/// `findResult`: `Some(true)` if the ACL allows this, `Some(false)` if it denies, `None` if it is not about
/// this principal, host or operation.
fn result_of(acl: &Acl, who: Subject<'_>, op: i8) -> Option<bool> {
    if acl.principal != who.principal && acl.principal != WILDCARD_PRINCIPAL {
        return None;
    }
    if acl.host != WILDCARD && acl.host != who.host {
        return None;
    }
    let allow = acl.permission == permission::ALLOW;
    if acl.operation != operation::ALL {
        // Only an ALLOW implies: a DENY of Read does not deny Describe.
        let applies = if allow {
            match op {
                operation::DESCRIBE => matches!(
                    acl.operation,
                    operation::DESCRIBE
                        | operation::READ
                        | operation::WRITE
                        | operation::DELETE
                        | operation::ALTER
                ),
                operation::DESCRIBE_CONFIGS => matches!(
                    acl.operation,
                    operation::DESCRIBE_CONFIGS | operation::ALTER_CONFIGS
                ),
                _ => acl.operation == op,
            }
        } else {
            acl.operation == op
        };
        if !applies {
            return None;
        }
    }
    Some(allow)
}
