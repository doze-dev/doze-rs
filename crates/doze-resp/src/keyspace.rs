//! The key browser's plain logic, apart from any server: keys split into folders on `:` (`user:1:name` sits in the
//! folder `user`, then `1`), the ids that name folders and keys in the tree, and patterns that match a key's own
//! characters even when they are glob characters.
//!
//! The ids: a folder is `f:` and its prefix (`f:user:1:`; the top is the empty id), a key is `k:` and the key
//! (`k:user:1`), and the note that a scan stopped early is `m:` and its folder's prefix. So the folder `user` and the
//! key `user` are told apart. A key or prefix that is not UTF-8 is written in hex after `f#`, `k#` or `m#`.

use std::collections::{BTreeMap, BTreeSet};

/// What splits a key into folders.
pub const SEP: u8 = b':';

/// What a tree id names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Id {
    /// A folder, by its prefix (ending in `:`; empty for the top).
    Folder(Vec<u8>),
    /// A key.
    Key(Vec<u8>),
    /// The note that the folder with this prefix was not scanned to its end.
    More(Vec<u8>),
}

impl Id {
    /// The id as the tree carries it.
    pub fn encode(&self) -> String {
        let (tag, b) = match self {
            Id::Folder(b) if b.is_empty() => return String::new(),
            Id::Folder(b) => ('f', b),
            Id::Key(b) => ('k', b),
            Id::More(b) => ('m', b),
        };
        match std::str::from_utf8(b) {
            Ok(s) => format!("{tag}:{s}"),
            Err(_) => format!("{tag}#{}", hex(b)),
        }
    }

    /// The id the tree was asked with; `None` for one this module never gave.
    pub fn parse(id: &str) -> Option<Id> {
        if id.is_empty() {
            return Some(Id::Folder(Vec::new()));
        }
        let (head, rest) = (id.get(..2)?, id.get(2..)?);
        let bytes = match head.as_bytes().get(1) {
            Some(b':') => rest.as_bytes().to_vec(),
            Some(b'#') => unhex(rest)?,
            _ => return None,
        };
        match head.as_bytes().first() {
            Some(b'f') if bytes.last() == Some(&SEP) => Some(Id::Folder(bytes)),
            Some(b'k') => Some(Id::Key(bytes)),
            Some(b'm') => Some(Id::More(bytes)),
            _ => None,
        }
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

/// `raw` with every glob character (`*`, `?`, `[`, `]`, `\`) escaped, so a `SCAN MATCH` pattern built from it
/// matches those characters themselves.
pub fn glob_escape(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len());
    for &b in raw {
        if matches!(b, b'*' | b'?' | b'[' | b']' | b'\\') {
            out.push(b'\\');
        }
        out.push(b);
    }
    out
}

/// The pattern for every key under `prefix`.
pub fn under(prefix: &[u8]) -> Vec<u8> {
    let mut p = glob_escape(prefix);
    p.push(b'*');
    p
}

/// The pattern for every key under `prefix` holding `filter` after it.
pub fn holding(prefix: &[u8], filter: &[u8]) -> Vec<u8> {
    let mut p = under(prefix);
    p.extend(glob_escape(filter));
    p.push(b'*');
    p
}

/// One name at a folder's level: a folder (the keys under it), a key, or both (`user` and `user:1`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Level {
    /// The name between the folder's prefix and the next `:` (or the key's end).
    pub segment: Vec<u8>,
    /// Keys under the folder of this name; 0 when there is no such folder.
    pub keys: u64,
    /// A key ends here.
    pub key: bool,
}

/// The names one level below `prefix` among `keys` (keys not under `prefix` are skipped), in byte order.
pub fn group<'a>(prefix: &[u8], keys: impl IntoIterator<Item = &'a Vec<u8>>) -> Vec<Level> {
    let mut at: BTreeMap<&[u8], (u64, bool)> = BTreeMap::new();
    for k in keys {
        let Some(rest) = k.strip_prefix(prefix) else {
            continue;
        };
        match rest.iter().position(|&b| b == SEP) {
            Some(i) => at.entry(rest.get(..i).unwrap_or_default()).or_default().0 += 1,
            None => at.entry(rest).or_default().1 = true,
        }
    }
    at.into_iter()
        .map(|(s, (keys, key))| Level {
            segment: s.to_vec(),
            keys,
            key,
        })
        .collect()
}

/// The keys a scan found, without the repeats `SCAN` may give.
pub type Found = BTreeSet<Vec<u8>>;

/// The folder a level's name opens (`prefix` + name + `:`) and the key it is (`prefix` + name).
pub fn ids(prefix: &[u8], segment: &[u8]) -> (Id, Id) {
    let mut key = prefix.to_vec();
    key.extend_from_slice(segment);
    let mut folder = key.clone();
    folder.push(SEP);
    (Id::Folder(folder), Id::Key(key))
}

/// Seconds as a person reads a time left: `45s`, `12m`, `3h 5m`, `2d 4h`.
pub fn human_secs(s: u64) -> String {
    let (d, h, m) = (s / 86_400, s / 3600 % 24, s / 60 % 60);
    match (d, h, m) {
        (0, 0, 0) => format!("{s}s"),
        (0, 0, m) if s.is_multiple_of(60) => format!("{m}m"),
        (0, 0, m) => format!("{m}m {}s", s % 60),
        (0, h, 0) => format!("{h}h"),
        (0, h, m) => format!("{h}h {m}m"),
        (d, 0, _) => format!("{d}d"),
        (d, h, _) => format!("{d}d {h}h"),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn found(keys: &[&str]) -> Found {
        keys.iter().map(|k| k.as_bytes().to_vec()).collect()
    }

    #[test]
    fn keys_group_by_their_next_segment_and_a_name_can_be_both() {
        let keys = found(&[
            "user",
            "user:1",
            "user:2:name",
            "a:b:c",
            "a:b:d",
            "a",
            "plain",
        ]);
        let top = group(b"", &keys);
        let seen: Vec<(&str, u64, bool)> = top
            .iter()
            .map(|l| (std::str::from_utf8(&l.segment).unwrap(), l.keys, l.key))
            .collect();
        assert_eq!(
            seen,
            [("a", 2, true), ("plain", 0, true), ("user", 2, true)]
        );
        let user = group(b"user:", &keys);
        assert_eq!(user.len(), 2);
        assert_eq!(
            (user[0].segment.as_slice(), user[0].key, user[0].keys),
            (&b"1"[..], true, 0)
        );
        assert_eq!(
            (user[1].segment.as_slice(), user[1].key, user[1].keys),
            (&b"2"[..], false, 1)
        );
        let ab = group(b"a:b:", &keys);
        assert_eq!(ab.iter().filter(|l| l.key).count(), 2);
        // A key ending in the separator is a key with an empty name in its folder.
        let empty = group(b"x:", &found(&["x:"]));
        assert_eq!((empty[0].segment.len(), empty[0].key), (0, true));
    }

    #[test]
    fn a_folder_and_a_key_of_the_same_name_have_different_ids() {
        let (folder, key) = ids(b"", b"user");
        assert_eq!(folder.encode(), "f:user:");
        assert_eq!(key.encode(), "k:user");
        assert_ne!(folder.encode(), key.encode());
        for id in [
            folder,
            key,
            Id::More(b"a:".to_vec()),
            Id::Key(b"\xff:\x00".to_vec()),
        ] {
            assert_eq!(Id::parse(&id.encode()), Some(id));
        }
        assert_eq!(Id::Key(b"\xff".to_vec()).encode(), "k#ff");
        assert_eq!(Id::parse(""), Some(Id::Folder(vec![])));
        assert_eq!(Id::Folder(vec![]).encode(), "");
        for bad in ["user", "f:user", "k#f", "k#zz", "x:1", "k"] {
            assert_eq!(Id::parse(bad), None, "{bad}");
        }
        assert_eq!(Id::parse("k:"), Some(Id::Key(vec![])));
    }

    #[test]
    fn glob_characters_in_a_name_match_only_themselves() {
        assert_eq!(glob_escape(br"a*b?c[d]e\f"), br"a\*b\?c\[d\]e\\f".to_vec());
        assert_eq!(under(b"user:"), b"user:*".to_vec());
        assert_eq!(under(b"*:"), br"\*:*".to_vec());
        assert_eq!(holding(b"", b"a?"), br"*a\?*".to_vec());
        assert_eq!(holding(b"q:", b"x"), b"q:*x*".to_vec());
    }

    #[test]
    fn a_time_left_reads_plainly() {
        assert_eq!(human_secs(45), "45s");
        assert_eq!(human_secs(720), "12m");
        assert_eq!(human_secs(100), "1m 40s");
        assert_eq!(human_secs(3600), "1h");
        assert_eq!(human_secs(3600 * 3 + 300), "3h 5m");
        assert_eq!(human_secs(86_400 * 2 + 3600 * 4), "2d 4h");
        assert_eq!(human_secs(86_400), "1d");
    }
}
