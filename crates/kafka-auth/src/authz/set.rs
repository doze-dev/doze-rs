//! The ACLs of a broker, indexed for lookup.

use std::collections::HashMap;

use super::{Acl, AclFilter, pattern, resource};

/// The ACLs of one resource type, split by how they name resources.
#[derive(Debug, Clone, Default)]
pub(super) struct ByType {
    /// LITERAL ACLs by the name they have (`*` among them).
    pub(super) literal: HashMap<String, Vec<Acl>>,
    /// PREFIXED ACLs: the resources they cover are found by testing each one's prefix.
    pub(super) prefixed: Vec<Acl>,
    /// Every ACL of the type, whatever its pattern.
    pub(super) all: Vec<Acl>,
}

/// An immutable set of ACLs. Built once per change and shared; a lookup never takes a lock.
#[derive(Debug, Clone, Default)]
pub struct AclSet {
    /// Every ACL, in order, each once.
    all: Vec<Acl>,
    /// Indexed by resource type (`resource::TOPIC..=resource::USER`; the entries 0 and 1 stay empty).
    types: Vec<ByType>,
}

impl AclSet {
    /// The set of `acls`; a duplicate counts once.
    #[must_use]
    pub fn new(acls: impl IntoIterator<Item = Acl>) -> Self {
        let mut all: Vec<Acl> = acls.into_iter().collect();
        all.sort();
        all.dedup();
        let mut types = vec![ByType::default(); usize::from(resource::USER.unsigned_abs()) + 1];
        for acl in &all {
            let Some(slot) = usize::try_from(acl.resource_type)
                .ok()
                .and_then(|i| types.get_mut(i))
            else {
                continue;
            };
            slot.all.push(acl.clone());
            match acl.pattern_type {
                pattern::LITERAL => slot
                    .literal
                    .entry(acl.name.clone())
                    .or_default()
                    .push(acl.clone()),
                pattern::PREFIXED => slot.prefixed.push(acl.clone()),
                _ => {}
            }
        }
        Self { all, types }
    }

    /// Whether there is no ACL at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.all.is_empty()
    }

    /// How many ACLs there are.
    #[must_use]
    pub fn len(&self) -> usize {
        self.all.len()
    }

    /// Every ACL, in order.
    pub fn iter(&self) -> impl Iterator<Item = &Acl> {
        self.all.iter()
    }

    /// The ACLs `filter` selects, in order.
    #[must_use]
    pub fn select(&self, filter: &AclFilter) -> Vec<&Acl> {
        self.all.iter().filter(|a| filter.matches(a)).collect()
    }

    /// The ACLs of resource type `kind`; none for a type that cannot be stored.
    pub(super) fn of_type(&self, kind: i8) -> Option<&ByType> {
        self.types.get(usize::try_from(kind).ok()?)
    }
}
