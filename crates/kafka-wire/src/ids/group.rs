//! Consumer groups: the names a client may give one.

use std::fmt;
use std::sync::Arc;

/// The longest group id the broker keeps. Kafka has no limit of its own; this one keeps a key in the metadata
/// database small, and is what the Go edition holds to.
pub const MAX_GROUP_ID_LEN: usize = 4096;

/// Why a string cannot be a group id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum GroupIdError {
    /// There is nothing in it.
    #[error("the group id is empty")]
    Empty,
    /// Longer than [`MAX_GROUP_ID_LEN`] bytes.
    #[error("the group id is longer than {MAX_GROUP_ID_LEN} bytes")]
    TooLong,
    /// A NUL byte, which separates the parts of a stored key.
    #[error("the group id contains a NUL byte")]
    Nul,
}

/// A consumer group's id: 1 to 4096 bytes, without a NUL. A `GroupId` cannot be made any other way than
/// [`GroupId::parse`], so the store's keys, which join it to other parts with a NUL, are never ambiguous.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GroupId(Arc<str>);

impl GroupId {
    /// A group id, if the broker can keep it.
    ///
    /// # Errors
    /// [`GroupIdError`] says which rule it breaks.
    pub fn parse(id: &str) -> Result<Self, GroupIdError> {
        if id.is_empty() {
            Err(GroupIdError::Empty)
        } else if id.len() > MAX_GROUP_ID_LEN {
            Err(GroupIdError::TooLong)
        } else if id.contains('\0') {
            Err(GroupIdError::Nul)
        } else {
            Ok(Self(id.into()))
        }
    }

    /// The id.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for GroupId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl AsRef<str> for GroupId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// A member's id within a group: the broker makes them (`doze-` and 32 hex digits), and a client sends back what it was
/// given. Any text; an empty one on the wire means "none yet" and is not a `MemberId`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MemberId(Arc<str>);

impl MemberId {
    /// A member id, or `None` for the wire's empty string.
    #[must_use]
    pub fn new(id: &str) -> Option<Self> {
        (!id.is_empty()).then(|| Self(id.into()))
    }

    /// The id.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for MemberId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// A static member's `group.instance.id`: it names a member across restarts. Any text; empty is not one.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InstanceId(Arc<str>);

impl InstanceId {
    /// An instance id, or `None` for the wire's empty string.
    #[must_use]
    pub fn new(id: &str) -> Option<Self> {
        (!id.is_empty()).then(|| Self(id.into()))
    }

    /// The id.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for InstanceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_id_is_one_to_4096_bytes_without_a_nul() {
        assert_eq!(GroupId::parse("g").unwrap().as_str(), "g");
        assert!(GroupId::parse("grüße 日本").is_ok(), "any text but NUL");
        assert_eq!(GroupId::parse(""), Err(GroupIdError::Empty));
        assert_eq!(GroupId::parse("a\0b"), Err(GroupIdError::Nul));
        assert!(GroupId::parse(&"x".repeat(MAX_GROUP_ID_LEN)).is_ok());
        assert_eq!(
            GroupId::parse(&"x".repeat(MAX_GROUP_ID_LEN + 1)),
            Err(GroupIdError::TooLong)
        );
        // The limit is in bytes, not characters.
        assert_eq!(
            GroupId::parse(&"é".repeat(MAX_GROUP_ID_LEN / 2 + 1)),
            Err(GroupIdError::TooLong)
        );
    }
}
