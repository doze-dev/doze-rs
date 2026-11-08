//! Topics and their partitions: the names a client may create and the numbers that address them.

use std::fmt;
use std::sync::Arc;

/// Kafka's limit on a topic name's length.
pub const MAX_TOPIC_NAME_LEN: usize = 249;

/// Why a name cannot be a topic's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum TopicNameError {
    /// There is nothing in it.
    #[error("Topic name is invalid: the empty string is not allowed")]
    Empty,
    /// `.` and `..` name directories.
    #[error("Topic name is invalid: '.' and '..' are not allowed")]
    Reserved,
    /// Longer than [`MAX_TOPIC_NAME_LEN`].
    #[error(
        "Topic name is invalid: the length of a topic name is limited to {MAX_TOPIC_NAME_LEN} characters"
    )]
    TooLong,
    /// A character outside ASCII letters, digits, `.`, `_` and `-`.
    #[error(
        "Topic name is invalid: it contains one or more characters other than ASCII alphanumerics, '.', '_' and '-'"
    )]
    BadCharacter,
}

/// A topic's name, as Kafka allows it to be created: 1 to 249 characters from `[a-zA-Z0-9._-]`, and not `.`
/// or `..`.
///
/// This is a security boundary as well as a compatibility one: a topic's name becomes a directory name under
/// the data directory, and the alphabet has no path separator in it. A `TopicName` cannot be made any other
/// way than [`TopicName::parse`], so storage never meets a name that could leave its directory.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TopicName(Arc<str>);

impl TopicName {
    /// A name, if Kafka would allow a topic to have it.
    ///
    /// # Errors
    /// [`TopicNameError`] says which rule it breaks.
    pub fn parse(name: &str) -> Result<Self, TopicNameError> {
        match name {
            "" => return Err(TopicNameError::Empty),
            "." | ".." => return Err(TopicNameError::Reserved),
            _ if name.len() > MAX_TOPIC_NAME_LEN => return Err(TopicNameError::TooLong),
            _ => {}
        }
        let allowed = |c: u8| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-');
        if name.bytes().all(allowed) {
            Ok(Self(name.into()))
        } else {
            Err(TopicNameError::BadCharacter)
        }
    }

    /// The name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TopicName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl AsRef<str> for TopicName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// A topic's identity for as long as it lives: deleting a topic and creating another of the same name gives
/// a new one, which is how a client tells them apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TopicId([u8; 16]);

impl TopicId {
    /// The id with these bytes.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    /// The id's bytes, as the protocol carries it.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

/// A partition's number within its topic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PartitionId(i32);

impl PartitionId {
    /// A partition number, which cannot be negative.
    #[must_use]
    pub fn new(v: i32) -> Option<Self> {
        (v >= 0).then_some(Self(v))
    }

    /// The number as the protocol carries it.
    #[must_use]
    pub fn get(self) -> i32 {
        self.0
    }
}

impl fmt::Display for PartitionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// How many partitions a topic has: at least one, and few enough that every partition has a number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PartitionCount(i32);

impl PartitionCount {
    /// A count, which is at least one.
    #[must_use]
    pub fn new(v: i32) -> Option<Self> {
        (v >= 1).then_some(Self(v))
    }

    /// The count as the protocol carries it.
    #[must_use]
    pub fn get(self) -> i32 {
        self.0
    }

    /// The partitions, in order.
    pub fn ids(self) -> impl Iterator<Item = PartitionId> {
        (0..self.0).map(PartitionId)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_is_what_kafka_allows_and_nothing_that_could_leave_a_directory() {
        for ok in ["a", "A.b_c-9", "..a", "a..", &"x".repeat(249)] {
            assert_eq!(TopicName::parse(ok).unwrap().as_str(), ok);
        }
        let cases = [
            ("", TopicNameError::Empty),
            (".", TopicNameError::Reserved),
            ("..", TopicNameError::Reserved),
            (&"x".repeat(250), TopicNameError::TooLong),
            ("../victim", TopicNameError::BadCharacter),
            ("a/b", TopicNameError::BadCharacter),
            ("a\\b", TopicNameError::BadCharacter),
            ("a b", TopicNameError::BadCharacter),
            ("a\0b", TopicNameError::BadCharacter),
            ("café", TopicNameError::BadCharacter),
        ];
        for (name, why) in cases {
            assert_eq!(TopicName::parse(name), Err(why), "{name:?}");
        }
    }

    #[test]
    fn partitions_are_not_negative() {
        assert_eq!(PartitionId::new(-1), None);
        assert_eq!(PartitionId::new(0).map(PartitionId::get), Some(0));
    }
}
