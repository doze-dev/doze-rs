//! The bytes items are filed under. A key attribute's value is written so that byte order is the order DynamoDB gives it
//! (strings and binary by their bytes, numbers by value) and so that it ends by itself: a partition key is a prefix of every
//! entry in its partition and of no other.

use aws_expr::Value;

/// The space an entry lives in: the table's items, a secondary index's entries, or a stream's records.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Space {
    Items,
    Index,
    Stream,
}

impl Space {
    pub(crate) const fn table_name(self) -> &'static str {
        match self {
            Self::Items => "items",
            Self::Index => "index",
            Self::Stream => "stream",
        }
    }
}

/// Writes a key value. A string or binary has zero escaped (`00` becomes `00 FF`) and ends in `00 01`, which sorts below every
/// continuation; a number is its order key.
pub(crate) fn encode(v: &Value, out: &mut Vec<u8>) {
    match v {
        Value::S(s) => escaped(s.as_bytes(), out),
        Value::B(b) => escaped(b, out),
        Value::N(n) => n.order_key(out),
        // WHY: only the three scalar types can be keys; the schema check keeps the others from getting here.
        _ => {}
    }
}

fn escaped(bytes: &[u8], out: &mut Vec<u8>) {
    for b in bytes {
        out.push(*b);
        if *b == 0 {
            out.push(0xFF);
        }
    }
    out.extend_from_slice(&[0, 1]);
}

/// The smallest key above everything that starts with `prefix`.
pub(crate) fn prefix_end(prefix: &[u8]) -> Vec<u8> {
    let mut end = prefix.to_vec();
    while let Some(last) = end.pop() {
        if last < 0xFF {
            end.push(last + 1);
            return end;
        }
    }
    // WHY: every byte was 0xFF; nothing sorts above such a prefix, so the bound is one byte longer than any key will be.
    vec![0xFF; prefix.len() + 1]
}

#[cfg(test)]
mod tests {
    use aws_expr::Number;

    use super::*;

    fn key(v: &Value) -> Vec<u8> {
        let mut out = Vec::new();
        encode(v, &mut out);
        out
    }

    #[test]
    fn strings_sort_by_bytes_and_a_prefix_is_below_its_extensions() {
        let order = ["", "a", "a\0", "a\0b", "ab", "b"];
        let keys: Vec<Vec<u8>> = order.iter().map(|s| key(&Value::S((*s).into()))).collect();
        for w in keys.windows(2) {
            assert!(w[0] < w[1]);
        }
    }

    #[test]
    fn a_partition_key_is_a_prefix_of_its_entries_only() {
        let a = key(&Value::S("a".into()));
        let mut entry = a.clone();
        encode(&Value::N(Number::from_i64(7)), &mut entry);
        assert!(entry.starts_with(&a));
        assert!(!key(&Value::S("ab".into())).starts_with(&a));
    }

    #[test]
    fn prefix_end_is_above_every_extension() {
        assert_eq!(prefix_end(&[1, 2, 0xFF]), vec![1, 3]);
        assert_eq!(prefix_end(&[0xFF]), vec![0xFF, 0xFF]);
        assert!(prefix_end(b"ab").as_slice() > b"ab\xFF\xFF".as_slice());
    }
}
