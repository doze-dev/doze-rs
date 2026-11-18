//! The classic consumer protocol's bytes as DescribeGroups hands them over: what a member subscribes to and what the leader
//! assigned it. The broker has the same decoder, but it is private to the group machines, and the console reads through the
//! protocol; so this is a second, small reader, held to the broker's own encoder by a test in `tests/live_groups.rs`.
//!
//! Subscription: version `i16`, topics (`i32` count, each `i16` length and bytes), user data, and from version 1 the owned
//! partitions. Assignment: version `i16`, topics each with partitions (`i32` count; name; `i32` count of `i32`s), user data.

use std::collections::BTreeMap;

use crate::model::GroupPart;

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

    /// A count the bytes left could hold at `each` bytes an element: a larger one is a lie, not a reason to allocate.
    fn count(&mut self, each: usize) -> Option<usize> {
        let n = usize::try_from(self.i32()?).ok()?;
        (n <= self.0.len() / each).then_some(n)
    }

    fn string(&mut self) -> Option<String> {
        let len = usize::try_from(self.i16()?).ok()?;
        String::from_utf8(self.take(len)?.to_vec()).ok()
    }
}

/// The partitions an assignment gives, topic by topic in order; `None` if the bytes are not one.
#[must_use]
pub fn assignment(bytes: &[u8]) -> Option<Vec<GroupPart>> {
    let mut r = Reader(bytes);
    r.i16()?;
    let mut by_topic: BTreeMap<String, Vec<i32>> = BTreeMap::new();
    for _ in 0..r.count(6)? {
        let topic = r.string()?;
        let mut ps = Vec::new();
        for _ in 0..r.count(4)? {
            ps.push(r.i32()?);
        }
        by_topic.entry(topic).or_default().extend(ps);
    }
    Some(
        by_topic
            .into_iter()
            .flat_map(|(topic, mut ps)| {
                ps.sort_unstable();
                ps.into_iter().map(move |partition| GroupPart {
                    topic: topic.clone(),
                    partition,
                })
            })
            .collect(),
    )
}

/// The topics a subscription names; `None` if the bytes are not one (versions 0 to 3).
#[must_use]
pub fn subscription(bytes: &[u8]) -> Option<Vec<String>> {
    let mut r = Reader(bytes);
    if !(0..=3).contains(&r.i16()?) {
        return None;
    }
    let mut topics = Vec::new();
    for _ in 0..r.count(2)? {
        topics.push(r.string()?);
    }
    Some(topics)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assignment_bytes(topics: &[(&str, &[i32])]) -> Vec<u8> {
        let mut b = vec![0, 3];
        b.extend((topics.len() as i32).to_be_bytes());
        for (name, ps) in topics {
            b.extend((name.len() as i16).to_be_bytes());
            b.extend(name.as_bytes());
            b.extend((ps.len() as i32).to_be_bytes());
            for p in *ps {
                b.extend(p.to_be_bytes());
            }
        }
        b.extend((-1_i32).to_be_bytes());
        b
    }

    #[test]
    fn an_assignment_is_read_sorted_by_topic_and_partition() {
        let got = assignment(&assignment_bytes(&[("b", &[3, 1]), ("a", &[0])])).unwrap();
        let flat: Vec<(String, i32)> = got.into_iter().map(|p| (p.topic, p.partition)).collect();
        assert_eq!(
            flat,
            vec![
                ("a".to_owned(), 0),
                ("b".to_owned(), 1),
                ("b".to_owned(), 3)
            ]
        );
    }

    #[test]
    fn bytes_that_are_not_an_assignment_or_a_subscription_are_not_read() {
        assert!(assignment(&[]).is_none());
        assert!(
            assignment(&[0, 0, 0x7f, 0xff, 0xff, 0xff]).is_none(),
            "billions of topics in no bytes"
        );
        let mut cut = assignment_bytes(&[("a", &[0, 1])]);
        cut.truncate(cut.len() - 9);
        assert!(assignment(&cut).is_none());
        assert!(
            subscription(&[0, 9, 0, 0, 0, 0]).is_none(),
            "an unknown version"
        );
        assert_eq!(
            subscription(&[0, 0, 0, 0, 0, 1, 0, 1, b'x', 0, 0, 0, 0]),
            Some(vec!["x".to_owned()])
        );
        assert!(subscription(b"x").is_none());
    }
}
