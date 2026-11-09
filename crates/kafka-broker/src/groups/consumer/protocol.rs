//! The classic consumer protocol's bytes, as a consumer group of the new protocol needs them for the classic members
//! it holds: what a classic consumer subscribes to (and, from version 1, the partitions it still owns), and the
//! assignment it is given.
//!
//! A subscription is: a version (`i16`), the topics (`i32` count, each a string with an `i16` length), user data
//! (`i32` length, -1 for none) and, from version 1, the owned partitions. An assignment is: a version, the topics
//! each with its partitions (`i32` count, a string, `i32` count of `i32`s), user data. Versions 0 to 3 share that layout.

use std::collections::BTreeMap;

use bytes::{BufMut, Bytes, BytesMut};

/// Partitions by topic name.
pub(crate) type ByName = BTreeMap<String, Vec<i32>>;

/// What a classic consumer's metadata says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Subscription {
    /// The version it was written at: assignments are given at the same one.
    pub(crate) version: i16,
    pub(crate) topics: Vec<String>,
    /// What it still owns (version 1 and later).
    pub(crate) owned: ByName,
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let (head, rest) = self.0.split_at_checked(n)?;
        self.0 = rest;
        Some(head)
    }

    fn i16(&mut self) -> Option<i16> {
        Some(i16::from_be_bytes(*self.take(2)?.first_chunk()?))
    }

    fn i32(&mut self) -> Option<i32> {
        Some(i32::from_be_bytes(*self.take(4)?.first_chunk()?))
    }

    /// A count that the bytes left could hold at `each` bytes an element: a larger one is a lie, not a reason to
    /// allocate.
    fn count(&mut self, each: usize) -> Option<usize> {
        let n = usize::try_from(self.i32()?).ok()?;
        (n <= self.0.len() / each).then_some(n)
    }

    fn string(&mut self) -> Option<String> {
        let len = usize::try_from(self.i16()?).ok()?;
        String::from_utf8(self.take(len)?.to_vec()).ok()
    }

    fn partitions(&mut self) -> Option<ByName> {
        let mut out = ByName::new();
        for _ in 0..self.count(6)? {
            let name = self.string()?;
            let mut ps = Vec::new();
            for _ in 0..self.count(4)? {
                ps.push(self.i32()?);
            }
            out.entry(name).or_default().extend(ps);
        }
        Some(out)
    }
}

/// A subscription, or `None` if the bytes are not one of version 0 to 3. A version 1 or later whose owned partitions
/// cannot be read is not read at all: taking it to own nothing would free partitions it may be reading.
#[must_use]
pub(crate) fn decode_subscription(bytes: &[u8]) -> Option<Subscription> {
    let mut r = Reader(bytes);
    let version = r.i16()?;
    if !(0..=3).contains(&version) {
        return None;
    }
    let mut topics = Vec::new();
    for _ in 0..r.count(2)? {
        topics.push(r.string()?);
    }
    if version < 1 {
        return Some(Subscription {
            version,
            topics,
            owned: ByName::new(),
        });
    }
    // User data: a length, then that many bytes (-1: none).
    let user = r.i32()?;
    if user > 0 {
        r.take(usize::try_from(user).ok()?)?;
    }
    let owned = r.partitions()?;
    Some(Subscription {
        version,
        topics,
        owned,
    })
}

/// A classic assignment as topics with partitions, or `None` if the bytes are not one.
#[must_use]
pub(crate) fn decode_assignment(bytes: &[u8]) -> Option<ByName> {
    let mut r = Reader(bytes);
    r.i16()?;
    r.partitions()
}

/// The bytes a classic member is given: the topics it owns with their partitions, no user data, at `version`.
#[must_use]
pub(crate) fn encode_assignment(version: i16, topics: &ByName) -> Bytes {
    let mut b = BytesMut::new();
    b.put_i16(version);
    b.put_i32(i32::try_from(topics.len()).unwrap_or(i32::MAX));
    for (name, partitions) in topics {
        let mut ps = partitions.clone();
        ps.sort_unstable();
        b.put_i16(i16::try_from(name.len()).unwrap_or(i16::MAX));
        b.put_slice(name.as_bytes());
        b.put_i32(i32::try_from(ps.len()).unwrap_or(i32::MAX));
        for p in ps {
            b.put_i32(p);
        }
    }
    b.put_i32(-1);
    b.freeze()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn subscription(version: i16, topics: &[&str], owned: &ByName) -> Vec<u8> {
        let mut b = BytesMut::new();
        b.put_i16(version);
        b.put_i32(i32::try_from(topics.len()).unwrap());
        for t in topics {
            b.put_i16(i16::try_from(t.len()).unwrap());
            b.put_slice(t.as_bytes());
        }
        b.put_i32(-1);
        if version >= 1 {
            let encoded = encode_assignment(0, owned);
            // An owned-partitions array has the assignment's layout without its version or user data.
            b.put_slice(&encoded[2..encoded.len() - 4]);
        }
        b.to_vec()
    }

    #[test]
    fn a_subscription_is_read_at_every_version_with_what_the_member_owns() {
        let owned: ByName = [("a".to_owned(), vec![0, 2]), ("b".to_owned(), vec![1])].into();
        for v in 0..=3 {
            let s = decode_subscription(&subscription(v, &["a", "b"], &owned)).unwrap();
            assert_eq!(
                (s.version, s.topics.clone()),
                (v, vec!["a".to_owned(), "b".to_owned()]),
                "v{v}"
            );
            assert_eq!(
                s.owned,
                if v >= 1 { owned.clone() } else { ByName::new() },
                "v{v}"
            );
        }
    }

    #[test]
    fn a_subscription_that_cannot_be_read_is_not_read_at_all() {
        let owned: ByName = [("a".to_owned(), vec![0])].into();
        assert!(
            decode_subscription(&subscription(4, &["a"], &owned)).is_none(),
            "a version this does not know"
        );
        let mut cut = subscription(1, &["a"], &owned);
        cut.truncate(cut.len() - 2);
        assert!(
            decode_subscription(&cut).is_none(),
            "owned partitions cut short are not taken to be none"
        );
        assert!(decode_subscription(&[]).is_none());
        assert!(
            decode_subscription(&[0, 1, 0x7f, 0xff, 0xff, 0xff]).is_none(),
            "two billion topics in no bytes"
        );
    }

    #[test]
    fn an_assignment_round_trips_and_is_sorted() {
        let t: ByName = [("b".to_owned(), vec![3, 1]), ("a".to_owned(), vec![0])].into();
        let bytes = encode_assignment(3, &t);
        let back = decode_assignment(&bytes).unwrap();
        assert_eq!(back.get("a"), Some(&vec![0]));
        assert_eq!(back.get("b"), Some(&vec![1, 3]));
        assert_eq!(&bytes[..2], &[0, 3]);
        assert_eq!(&bytes[bytes.len() - 4..], &[255, 255, 255, 255]);
        // The recorded bytes of an empty assignment at version 0.
        assert_eq!(
            encode_assignment(0, &ByName::new()).as_ref(),
            &[0, 0, 0, 0, 0, 0, 255, 255, 255, 255]
        );
    }
}
