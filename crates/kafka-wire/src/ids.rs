//! Domain numbers with their own types, so an offset is never a timestamp or a producer id.

use std::fmt;

/// A position in a partition's log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Offset(i64);

impl Offset {
    /// The first offset of every partition.
    pub const ZERO: Offset = Offset(0);

    /// An offset, which cannot be negative.
    #[must_use]
    pub fn new(v: i64) -> Option<Self> {
        (v >= 0).then_some(Self(v))
    }

    /// The offset as the protocol carries it.
    #[must_use]
    pub fn get(self) -> i64 {
        self.0
    }

    /// This offset advanced by `n` records, or `None` if that would overflow.
    #[must_use]
    pub fn checked_add(self, n: u32) -> Option<Self> {
        self.0.checked_add(i64::from(n)).map(Self)
    }
}

impl fmt::Display for Offset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// The id of an idempotent or transactional producer. (On the wire, -1 means there is none: that is
/// an `Option<ProducerId>`, not a value.)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProducerId(i64);

impl ProducerId {
    /// A producer id, or `None` for the wire's "no producer" (any negative number).
    #[must_use]
    pub fn new(v: i64) -> Option<Self> {
        (v >= 0).then_some(Self(v))
    }

    /// The id as the protocol carries it.
    #[must_use]
    pub fn get(self) -> i64 {
        self.0
    }
}

/// A producer's epoch; it only ever goes up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProducerEpoch(i16);

impl ProducerEpoch {
    /// An epoch as read from the wire.
    #[must_use]
    pub fn from_wire(v: i16) -> Self {
        Self(v)
    }

    /// The epoch as the protocol carries it.
    #[must_use]
    pub fn get(self) -> i16 {
        self.0
    }
}

/// A time in milliseconds since the Unix epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TimestampMs(i64);

impl TimestampMs {
    /// A time as read from the wire.
    #[must_use]
    pub fn from_wire(v: i64) -> Self {
        Self(v)
    }

    /// The time as the protocol carries it.
    #[must_use]
    pub fn get(self) -> i64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets_are_not_negative_and_do_not_overflow() {
        assert_eq!(Offset::new(-1), None);
        assert_eq!(
            Offset::new(5)
                .and_then(|o| o.checked_add(3))
                .map(Offset::get),
            Some(8)
        );
        assert_eq!(Offset::new(i64::MAX).and_then(|o| o.checked_add(1)), None);
    }

    #[test]
    fn minus_one_is_no_producer() {
        assert_eq!(ProducerId::new(-1), None);
        assert_eq!(ProducerId::new(0).map(ProducerId::get), Some(0));
    }
}
