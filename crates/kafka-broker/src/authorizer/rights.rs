//! What one caller may DESCRIBE among the topics, kept by a consumer group for as long as a member holds a regular
//! expression.
//!
//! Kafka resolves a regex subscription once (`TopicRegexResolver.resolveRegularExpressions`) and then removes from
//! each match every topic the requester may not DESCRIBE (`filterTopicDescribeAuthorizedTopics`, 125-166). The group
//! machine here has no principal and asks nothing of the authorizer, so the handler hands it the rights of the
//! request as a value: the ACLs in force when the request came, the rules, and the caller. `None` stands for a caller
//! that is not narrowed at all (the default allows and no ACL is stored, or a super user), and costs nothing.

use std::sync::Arc;

use kafka_auth::authz::{AclSet, Resource, Rules, operation, resource};

use crate::Caller;

/// The right to DESCRIBE topics, of one caller, against one set of ACLs.
#[derive(Debug, Clone)]
pub(crate) struct TopicRights {
    set: Arc<AclSet>,
    rules: Arc<Rules>,
    who: Caller,
}

impl TopicRights {
    pub(super) fn new(set: Arc<AclSet>, rules: Arc<Rules>, who: &Caller) -> Self {
        Self {
            set,
            rules,
            who: who.clone(),
        }
    }

    /// May the caller DESCRIBE the topic `name`?
    #[must_use]
    pub(crate) fn may_describe(&self, name: &str) -> bool {
        let topic = Resource {
            kind: resource::TOPIC,
            name,
        };
        self.rules
            .authorize(&self.set, self.who.subject(), topic, operation::DESCRIBE)
    }
}

/// The same caller against the very same ACLs and rules: then the answer to every question is the same.
impl PartialEq for TopicRights {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.set, &other.set)
            && Arc::ptr_eq(&self.rules, &other.rules)
            && self.who == other.who
    }
}

/// May a caller with these rights (`None`: unnarrowed) DESCRIBE the topic?
#[must_use]
pub(crate) fn permits(rights: Option<&TopicRights>, name: &str) -> bool {
    rights.is_none_or(|r| r.may_describe(name))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use kafka_auth::Principal;

    use super::super::Authorizer;
    use super::*;

    fn caller(name: &str) -> Caller {
        Caller::new(Principal::user(name), None)
    }

    #[test]
    fn nobody_is_narrowed_when_the_default_allows_and_nothing_is_stored_or_for_a_super_user() {
        let open = Authorizer::new(Vec::new(), true, BTreeSet::new());
        assert!(open.describe_rights(&caller("alice")).is_none());
        let closed = Authorizer::new(Vec::new(), false, BTreeSet::from(["User:root".to_owned()]));
        assert!(closed.describe_rights(&caller("root")).is_none());
        let narrowed = closed.describe_rights(&caller("alice"));
        assert!(narrowed.is_some_and(|r| !r.may_describe("t")));
    }

    #[test]
    fn rights_are_the_same_until_the_acls_or_the_caller_change() {
        let a = Authorizer::new(Vec::new(), false, BTreeSet::new());
        let first = a.describe_rights(&caller("alice"));
        assert_eq!(first, a.describe_rights(&caller("alice")));
        assert_ne!(first, a.describe_rights(&caller("bob")));
        a.replace(kafka_auth::authz::AclSet::default());
        assert_ne!(first, a.describe_rights(&caller("alice")));
        assert!(permits(None, "anything") && !permits(first.as_ref(), "anything"));
    }
}
