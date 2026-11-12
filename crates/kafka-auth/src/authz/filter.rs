//! The filter DescribeAcls and DeleteAcls select ACLs with: Kafka's `AclBindingFilter`.

use super::{Acl, WILDCARD, operation, pattern, permission, resource};

/// Why a filter cannot be used, in the words Kafka's DeleteAcls answers with (INVALID_REQUEST).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum FilterError {
    /// The resource type or the pattern type is no value of its enumeration.
    #[error("Unknown patternFilter.")]
    Pattern,
    /// The operation or the permission type is no value of its enumeration.
    #[error("Unknown entryFilter.")]
    Entry,
}

/// What to match. A `None` string and the ANY values match everything; the pattern type MATCH on a name matches
/// every ACL that would apply to that name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AclFilter {
    /// A resource type, or ANY.
    pub resource_type: i8,
    /// A resource name; `None` is any.
    pub name: Option<String>,
    /// A pattern type, ANY, or MATCH.
    pub pattern_type: i8,
    /// A principal; `None` is any.
    pub principal: Option<String>,
    /// A host; `None` is any. (`*` matches only ACLs whose host is the literal `*`.)
    pub host: Option<String>,
    /// An operation, or ANY.
    pub operation: i8,
    /// A permission type, or ANY.
    pub permission: i8,
}

impl AclFilter {
    /// The filter that matches every ACL.
    #[must_use]
    pub fn any() -> Self {
        Self {
            resource_type: resource::ANY,
            name: None,
            pattern_type: pattern::ANY,
            principal: None,
            host: None,
            operation: operation::ANY,
            permission: permission::ANY,
        }
    }

    /// Whether one of the four codes is 0, UNKNOWN: Kafka closes the connection on such a request while reading it.
    #[must_use]
    pub fn has_zero_code(&self) -> bool {
        [
            self.resource_type,
            self.pattern_type,
            self.operation,
            self.permission,
        ]
        .contains(&0)
    }

    // fuzz:parser acl_filter
    /// Whether the filter can be used: every code must be a value of its enumeration (a number that is none
    /// reads as UNKNOWN). A filter that cannot is refused by DeleteAcls and matches nothing in DescribeAcls.
    ///
    /// # Errors
    /// [`FilterError::Pattern`] for the resource or pattern type, else [`FilterError::Entry`] for the operation or
    /// permission.
    pub fn validate(&self) -> Result<(), FilterError> {
        if !resource::is_known(self.resource_type) || !pattern::is_known(self.pattern_type) {
            return Err(FilterError::Pattern);
        }
        if !operation::is_known(self.operation) || !permission::is_known(self.permission) {
            return Err(FilterError::Entry);
        }
        Ok(())
    }

    // fuzz:parser acl_filter
    /// Whether `acl` is selected: `AclBindingFilter.matches`, the pattern filter and the entry filter, both exact
    /// and case-sensitive.
    #[must_use]
    pub fn matches(&self, acl: &Acl) -> bool {
        self.matches_pattern(acl)
            && self.principal.as_ref().is_none_or(|p| *p == acl.principal)
            && self.host.as_ref().is_none_or(|h| *h == acl.host)
            && (self.operation == operation::ANY || self.operation == acl.operation)
            && (self.permission == permission::ANY || self.permission == acl.permission)
    }

    /// `ResourcePatternFilter.matches`.
    fn matches_pattern(&self, acl: &Acl) -> bool {
        if self.resource_type != resource::ANY && self.resource_type != acl.resource_type {
            return false;
        }
        if !matches!(self.pattern_type, pattern::ANY | pattern::MATCH)
            && self.pattern_type != acl.pattern_type
        {
            return false;
        }
        let Some(name) = &self.name else {
            return true;
        };
        if self.pattern_type == pattern::ANY || self.pattern_type == acl.pattern_type {
            return *name == acl.name;
        }
        // The filter is MATCH: which ACLs would apply to a resource called `name`.
        match acl.pattern_type {
            pattern::LITERAL => *name == acl.name || acl.name == WILDCARD,
            pattern::PREFIXED => name.starts_with(&acl.name),
            _ => false,
        }
    }
}
