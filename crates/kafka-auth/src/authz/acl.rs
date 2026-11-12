//! One access control entry, with the codes exactly as a client sent them.

use super::{CLUSTER_NAME, WILDCARD, operation, pattern, permission, resource};

/// An ACL: `principal` is `permission` to do `operation` from `host` on the resources `resource_type` that
/// `name` covers according to `pattern_type`. The four codes are the wire numbers, not enums, so what is stored
/// is what a client reads back.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Acl {
    /// The resource type (`codes::resource`).
    pub resource_type: i8,
    /// The resource name, or a prefix of names.
    pub name: String,
    /// How `name` covers resources (`codes::pattern`).
    pub pattern_type: i8,
    /// The principal as `Type:name`, or `User:*`.
    pub principal: String,
    /// `*`, or the textual address of the client.
    pub host: String,
    /// The operation (`codes::operation`).
    pub operation: i8,
    /// Whether it permits or refuses (`codes::permission`).
    pub permission: i8,
}

impl Acl {
    /// Whether the ACL's resource pattern covers the resource called `name` (of the right type already): a LITERAL
    /// ACL covers the name it has, or every name if it is `*`; a PREFIXED one every name that starts with its own.
    #[must_use]
    pub fn covers(&self, name: &str) -> bool {
        match self.pattern_type {
            pattern::LITERAL => self.name == name || self.name == WILDCARD,
            pattern::PREFIXED => name.starts_with(&self.name),
            _ => false,
        }
    }

    /// Whether one of the four codes is 0, UNKNOWN. Kafka refuses such a request while reading it and closes
    /// the connection, before anything is authorized or stored.
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

    /// Whether a code is a value only a filter can have (ANY for a type, an operation or a permission; ANY or
    /// MATCH for a pattern): Kafka cannot even build such an entry, and fails the whole request without a word.
    #[must_use]
    pub fn has_filter_only_code(&self) -> bool {
        self.resource_type == resource::ANY
            || matches!(self.pattern_type, pattern::ANY | pattern::MATCH)
            || self.operation == operation::ANY
            || self.permission == permission::ANY
    }

    /// Why Kafka refuses to store this entry (as INVALID_REQUEST), in its words, or `None` if it can be stored.
    /// The checks run in Kafka's order: the handler's (the CLUSTER's one name, an empty name), then the
    /// controller's (each code, then the principal). Any code that is no value of its enumeration reads as UNKNOWN.
    #[must_use]
    pub fn refusal(&self) -> Option<String> {
        if self.resource_type == resource::CLUSTER && self.name != CLUSTER_NAME {
            return Some(format!(
                "The only valid name for the CLUSTER resource is {CLUSTER_NAME}"
            ));
        }
        if self.name.is_empty() {
            return Some("Invalid empty resource name".to_owned());
        }
        if !(resource::TOPIC..=resource::USER).contains(&self.resource_type) {
            return Some("Invalid resourceType UNKNOWN".to_owned());
        }
        if !matches!(self.pattern_type, pattern::LITERAL | pattern::PREFIXED) {
            return Some("Invalid patternType UNKNOWN".to_owned());
        }
        if !(operation::ALL..=operation::TWO_PHASE_COMMIT).contains(&self.operation) {
            return Some("Invalid operation UNKNOWN".to_owned());
        }
        if !matches!(self.permission, permission::DENY | permission::ALLOW) {
            return Some("Invalid permissionType UNKNOWN".to_owned());
        }
        if !self.principal.contains(':') {
            return Some(format!(
                "Could not parse principal from `{}` (no colon is present separating the principal type from the principal name)",
                self.principal
            ));
        }
        None
    }
}
