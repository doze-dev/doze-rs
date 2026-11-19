//! A module's registry address, `<namespace>/<name>`: `valkey` is `doze/valkey`. Parsed once, so a name can never
//! become a path outside the registry or the cache (`..`, separators, empty parts are refused).

use std::fmt;

/// The namespace every module of doze's own lives in.
pub const DEFAULT_NAMESPACE: &str = "doze";

/// A checked `<namespace>/<name>`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Source {
    ns: String,
    name: String,
}

impl Source {
    /// `valkey`, or `ns/valkey`. Lowercase letters, digits, `-` and `_` only.
    pub fn parse(s: &str) -> Option<Source> {
        let (ns, name) = s.split_once('/').unwrap_or((DEFAULT_NAMESPACE, s));
        (part(ns) && part(name)).then(|| Source {
            ns: ns.to_string(),
            name: name.to_string(),
        })
    }

    pub fn ns(&self) -> &str {
        &self.ns
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// The path of one of its files in the registry (and the cache): `doze/valkey/index.yaml`.
    pub fn file(&self, file: &str) -> String {
        format!("{}/{}/{file}", self.ns, self.name)
    }

    /// How a person names it: `valkey` for doze's own, `ns/name` otherwise.
    pub fn short(&self) -> String {
        if self.ns == DEFAULT_NAMESPACE {
            self.name.clone()
        } else {
            self.to_string()
        }
    }
}

fn part(p: &str) -> bool {
    !p.is_empty()
        && p.len() <= 64
        && p.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
        && !p.starts_with(['-', '_'])
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.ns, self.name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_plain_names_become_sources() {
        assert_eq!(
            Source::parse("valkey").map(|s| s.to_string()).as_deref(),
            Some("doze/valkey")
        );
        assert_eq!(
            Source::parse("acme/kv-2").map(|s| s.short()).as_deref(),
            Some("acme/kv-2")
        );
        for bad in [
            "", "..", "../etc", "a/b/c", "Valkey", "a/", "/a", "-x", "a b",
        ] {
            assert_eq!(Source::parse(bad), None, "{bad:?}");
        }
    }
}
