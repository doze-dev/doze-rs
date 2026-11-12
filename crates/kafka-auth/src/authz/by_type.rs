//! `authorizeByResourceType`: may a principal do an operation on at least one resource of a type?
//!
//! Kafka uses it for one question, InitProducerId with no transactional id: a producer that may Write to any topic
//! may be handed a producer id even with no IdempotentWrite on the cluster (`Authorizer.java` 184-293).

use std::collections::HashSet;

use super::{
    Acl, AclSet, Resource, Rules, Subject, WILDCARD, WILDCARD_PRINCIPAL, operation, pattern,
    permission,
};

/// The name checked first, so that a super user (and the default, when it allows) passes whatever the ACLs say.
const HARDCODED: &str = "hardcode";

impl Rules {
    /// Whether `who` may do `op` on some resource of type `kind`.
    ///
    /// First the operation on a resource called `hardcode` is authorized as usual (super users pass; so does everyone
    /// when the default allows and no ACL covers that name). Failing that, the ACLs of the type that are for this
    /// principal and host, and whose operation is `op` or ALL (nothing implied), are collected: a DENY of `*`
    /// refuses at once, an ALLOW of `*` permits, and an ALLOW of a name or prefix permits unless a DENY covers it
    /// (the same name denied, or a denied prefix of it, itself included). The default does not apply beyond
    /// `hardcode`.
    #[must_use]
    pub fn authorize_by_resource_type(
        &self,
        set: &AclSet,
        who: Subject<'_>,
        op: i8,
        kind: i8,
    ) -> bool {
        if self.authorize(
            set,
            who,
            Resource {
                kind,
                name: HARDCODED,
            },
            op,
        ) {
            return true;
        }
        let Some(by_type) = set.of_type(kind) else {
            return false;
        };
        let mut deny_literal: HashSet<&str> = HashSet::new();
        let mut deny_prefix: HashSet<&str> = HashSet::new();
        let mut allow: Vec<&Acl> = Vec::new();
        let mut wildcard_allow = false;
        for acl in by_type.all.iter().filter(|a| for_subject(a, who, op)) {
            let denies = acl.permission == permission::DENY;
            match (acl.pattern_type, denies) {
                (pattern::LITERAL, true) if acl.name == WILDCARD => return false,
                (pattern::LITERAL, true) => {
                    deny_literal.insert(&acl.name);
                }
                (pattern::PREFIXED, true) => {
                    deny_prefix.insert(&acl.name);
                }
                (pattern::LITERAL, false) if acl.permission == permission::ALLOW => {
                    if acl.name == WILDCARD {
                        wildcard_allow = true;
                    } else {
                        allow.push(acl);
                    }
                }
                (pattern::PREFIXED, false) if acl.permission == permission::ALLOW => {
                    allow.push(acl)
                }
                _ => {}
            }
        }
        wildcard_allow
            || allow.iter().any(|acl| {
                let literal = acl.pattern_type == pattern::LITERAL;
                !(literal && deny_literal.contains(acl.name.as_str()))
                    && !prefixes(&acl.name).any(|p| deny_prefix.contains(p))
            })
    }
}

/// Whether the ACL is for this principal, from this host, about exactly this operation (or ALL).
fn for_subject(acl: &Acl, who: Subject<'_>, op: i8) -> bool {
    (acl.host == WILDCARD || acl.host == who.host)
        && (acl.principal == who.principal || acl.principal == WILDCARD_PRINCIPAL)
        && (acl.operation == op || acl.operation == operation::ALL)
}

/// Every non-empty prefix of `name`, the shortest first, `name` itself last.
fn prefixes(name: &str) -> impl Iterator<Item = &str> {
    name.char_indices()
        .map(|(i, c)| i + c.len_utf8())
        .filter_map(|end| name.get(..end))
}
