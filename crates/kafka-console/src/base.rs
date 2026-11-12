//! Where the console lives: every link is built from a base, so `--console-prefix` works everywhere or nowhere.

/// The path prefix the console is served under (`/` by default, `/kafka` behind a proxy that strips nothing).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Base {
    /// No trailing slash; empty for the root.
    prefix: String,
}

impl Base {
    /// The base for a `--console-prefix` value: slashes at either end are not significant (`kafka`, `/kafka/` and
    /// `/kafka` are one prefix), and `/` or nothing is the root.
    #[must_use]
    pub fn new(prefix: &str) -> Self {
        let inner = prefix.trim_matches('/');
        Self {
            prefix: if inner.is_empty() {
                String::new()
            } else {
                format!("/{inner}")
            },
        }
    }

    /// The root.
    #[must_use]
    pub fn root() -> Self {
        Self::new("/")
    }

    /// The URL of `path` (which starts with a slash) under this base.
    #[must_use]
    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.prefix)
    }

    /// The path of a request under this base, if it is under it: `/kafka/topics` is `/topics`, and `/kafkaesque` is
    /// nobody's.
    #[must_use]
    pub fn strip<'a>(&self, path: &'a str) -> Option<&'a str> {
        if self.prefix.is_empty() {
            return Some(path);
        }
        let rest = path.strip_prefix(self.prefix.as_str())?;
        match rest {
            "" => Some("/"),
            r if r.starts_with('/') => Some(r),
            _ => None,
        }
    }

    /// The prefix, as written in a URL (empty for the root).
    #[must_use]
    pub fn prefix(&self) -> &str {
        &self.prefix
    }
}

/// A path segment made safe to put in a URL: every byte that is not an unreserved character is `%XX`.
#[must_use]
pub fn segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    // `.` and `..` are directory steps to a browser, whatever the server makes of them.
    if out == "." || out == ".." {
        out = out.replace('.', "%2E");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_segment_keeps_only_what_a_path_may_hold_and_encodes_the_rest() {
        assert_eq!(segment("orders"), "orders");
        assert_eq!(segment("a.b-c_d~e"), "a.b-c_d~e");
        assert_eq!(segment("a/b?c#d e%"), "a%2Fb%3Fc%23d%20e%25");
        assert_eq!(segment("é"), "%C3%A9");
        assert_eq!(segment(".."), "%2E%2E");
        assert_eq!(segment("."), "%2E");
        assert_eq!(segment("..a"), "..a");
    }

    #[test]
    fn a_prefix_is_one_path_whatever_slashes_it_was_written_with() {
        for p in ["kafka", "/kafka", "kafka/", "/kafka/", "//kafka//"] {
            assert_eq!(Base::new(p).prefix(), "/kafka", "{p}");
        }
        for p in ["", "/", "//"] {
            assert_eq!(Base::new(p).prefix(), "", "{p}");
        }
    }

    #[test]
    fn a_url_is_the_prefix_then_the_path() {
        assert_eq!(Base::root().url("/connect"), "/connect");
        assert_eq!(Base::root().url("/"), "/");
        assert_eq!(Base::new("/k").url("/connect"), "/k/connect");
        assert_eq!(Base::new("/k").url("/"), "/k/");
    }

    #[test]
    fn a_request_path_is_stripped_of_the_prefix_and_only_of_whole_segments() {
        let b = Base::new("/k");
        assert_eq!(b.strip("/k/connect"), Some("/connect"));
        assert_eq!(b.strip("/k"), Some("/"));
        assert_eq!(b.strip("/k/"), Some("/"));
        assert_eq!(b.strip("/kafka"), None);
        assert_eq!(b.strip("/connect"), None);
        assert_eq!(Base::root().strip("/connect"), Some("/connect"));
    }
}
