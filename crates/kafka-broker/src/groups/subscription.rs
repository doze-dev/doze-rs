//! What a consumer says it subscribes to, read from the metadata it joins with.
//!
//! The consumer protocol's subscription is: a version (an `i16`), the topics (an `i32` count, each a string with an
//! `i16` length), and user data. Versions 0 to 3 share that start; a later version, or bytes that do not fit it, are
//! unreadable, and nothing is then assumed of the member's subscription.

/// The topics in `metadata`, or `None` if it is not a subscription this version can read.
#[must_use]
pub(crate) fn topics(metadata: &[u8]) -> Option<Vec<String>> {
    let (version, rest) = metadata.split_first_chunk::<2>()?;
    if !(0..=3).contains(&i16::from_be_bytes(*version)) {
        return None;
    }
    let (count, mut rest) = rest.split_first_chunk::<4>()?;
    let count = usize::try_from(i32::from_be_bytes(*count)).ok()?;
    // A count the bytes cannot hold is a lie, not a reason to allocate.
    if count > rest.len() / 2 {
        return None;
    }
    let mut topics = Vec::with_capacity(count);
    for _ in 0..count {
        let (len, after) = rest.split_first_chunk::<2>()?;
        let len = usize::try_from(i16::from_be_bytes(*len)).ok()?;
        let (name, after) = after.split_at_checked(len)?;
        topics.push(String::from_utf8(name.to_vec()).ok()?);
        rest = after;
    }
    Some(topics)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(version: i16, topics: &[&str]) -> Vec<u8> {
        let mut out = version.to_be_bytes().to_vec();
        out.extend_from_slice(&i32::try_from(topics.len()).unwrap().to_be_bytes());
        for t in topics {
            out.extend_from_slice(&i16::try_from(t.len()).unwrap().to_be_bytes());
            out.extend_from_slice(t.as_bytes());
        }
        out.extend_from_slice(&(-1_i32).to_be_bytes()); // no user data
        out
    }

    #[test]
    fn the_topics_of_versions_zero_to_three_are_read() {
        for v in 0..=3 {
            assert_eq!(
                topics(&encode(v, &["a", "bb"])),
                Some(vec!["a".into(), "bb".into()])
            );
        }
        assert_eq!(topics(&encode(1, &[])), Some(vec![]));
        // What a real client sends (recorded): version 0, one topic "dgo", null user data.
        let recorded = [0, 0, 0, 0, 0, 1, 0, 3, b'd', b'g', b'o', 255, 255, 255, 255];
        assert_eq!(topics(&recorded), Some(vec!["dgo".into()]));
    }

    #[test]
    fn anything_else_is_unreadable_and_a_false_count_allocates_nothing() {
        assert_eq!(
            topics(&encode(4, &["a"])),
            None,
            "a version this does not know"
        );
        assert_eq!(topics(&encode(-1, &["a"])), None);
        assert_eq!(topics(&[]), None);
        assert_eq!(topics(&[0, 0, 0, 0]), None, "no count");
        assert_eq!(
            topics(&[0, 0, 0x7f, 0xff, 0xff, 0xff]),
            None,
            "two billion topics in no bytes"
        );
        assert_eq!(
            topics(&[0, 0, 0, 0, 0, 1, 0, 9, b'x']),
            None,
            "a name longer than what is left"
        );
        assert_eq!(
            topics(&[0, 0, 0, 0, 0, 1, 0xff, 0xff]),
            None,
            "a negative length"
        );
        assert_eq!(topics(&[0, 0, 0, 0, 0, 1, 0, 1, 0xff]), None, "not UTF-8");
    }
}
