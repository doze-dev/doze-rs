//! What the broker lets each caller do: the ACLs it holds, and the rules to apply them with.
//!
//! The ACLs are an immutable snapshot behind an `ArcSwap`: a check loads it (one atomic load) and never waits,
//! and a change is built aside and swapped in by whoever holds the broker's ACL lock. With nothing stored and
//! the default allowing, a check is that load and a length test, which is the whole cost of leaving this
//! feature unused.

use std::collections::BTreeSet;
use std::sync::Arc;

use arc_swap::ArcSwap;
use kafka_auth::authz::{Acl, AclSet, Resource, Rules};
use kafka_meta::AclRecord;

use crate::Caller;

/// The authorizer of one broker.
#[derive(Debug)]
pub(crate) struct Authorizer {
    set: ArcSwap<AclSet>,
    rules: Rules,
}

impl Authorizer {
    pub(crate) fn new(
        stored: Vec<AclRecord>,
        default_allow: bool,
        super_users: BTreeSet<String>,
    ) -> Self {
        Self {
            set: ArcSwap::from_pointee(set_of(stored)),
            rules: Rules {
                default_allow,
                super_users,
            },
        }
    }

    /// Whether `who` may do `op` on the resource.
    pub(crate) fn allows(&self, who: &Caller, kind: i8, name: &str, op: i8) -> bool {
        let set = self.set.load();
        if set.is_empty() && self.rules.default_allow {
            return true;
        }
        self.rules
            .authorize(&set, who.subject(), Resource { kind, name }, op)
    }

    /// Whether `who` may do `op` on at least one resource of type `kind`.
    pub(crate) fn allows_some(&self, who: &Caller, op: i8, kind: i8) -> bool {
        let set = self.set.load();
        if set.is_empty() && self.rules.default_allow {
            return true;
        }
        self.rules
            .authorize_by_resource_type(&set, who.subject(), op, kind)
    }

    /// The ACLs now in force.
    pub(crate) fn snapshot(&self) -> Arc<AclSet> {
        self.set.load_full()
    }

    /// Puts `set` in force. Callers hold the broker's ACL lock, so two changes are never built from the same
    /// snapshot.
    pub(crate) fn replace(&self, set: AclSet) {
        self.set.store(Arc::new(set));
    }
}

fn set_of(stored: Vec<AclRecord>) -> AclSet {
    AclSet::new(stored.into_iter().map(acl_of))
}

pub(crate) fn acl_of(r: AclRecord) -> Acl {
    Acl {
        resource_type: r.resource_type,
        name: r.name,
        pattern_type: r.pattern_type,
        principal: r.principal,
        host: r.host,
        operation: r.operation,
        permission: r.permission,
    }
}

pub(crate) fn record_of(a: &Acl) -> AclRecord {
    AclRecord {
        resource_type: a.resource_type,
        name: a.name.clone(),
        pattern_type: a.pattern_type,
        principal: a.principal.clone(),
        host: a.host.clone(),
        operation: a.operation,
        permission: a.permission,
    }
}
