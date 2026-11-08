//! Consumer groups: the names a client may give one.

use std::fmt::{self, Write};
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

    /// An id the broker makes: `doze-` and the 16 bytes in hex, behind the instance id and a dash for a static
    /// member. It is never empty.
    #[must_use]
    pub fn issued(entropy: [u8; 16], instance: Option<&InstanceId>) -> Self {
        let mut text = String::new();
        if let Some(i) = instance {
            text.push_str(i.as_str());
            text.push('-');
        }
        text.push_str("doze-");
        for b in entropy {
            // Writing to a string cannot fail.
            let _ = write!(text, "{b:02x}");
        }
        Self(text.into())
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

/// A static member's `group.instance.id`: it names a member across restarts. Any text, the empty string too: a client
/// that sends one at all (not null) is a static member, and Kafka takes it as that.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InstanceId(Arc<str>);

impl InstanceId {
    /// An instance id.
    #[must_use]
    pub fn new(id: &str) -> Self {
        Self(id.into())
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

    #[test]
    fn an_issued_member_id_is_doze_and_hex_behind_the_instance_for_a_static_member() {
        let bytes = [0xab; 16];
        assert_eq!(
            MemberId::issued(bytes, None).as_str(),
            format!("doze-{}", "ab".repeat(16))
        );
        let inst = InstanceId::new("pod-7");
        assert_eq!(
            MemberId::issued([0; 16], Some(&inst)).as_str(),
            format!("pod-7-doze-{}", "00".repeat(16))
        );
    }
}
