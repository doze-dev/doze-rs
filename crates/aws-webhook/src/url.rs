//! An `http://` or `https://` URL taken apart: scheme, host, port, and the path with its query.

/// The parts a request needs.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Target<'a> {
    pub(crate) tls: bool,
    /// The host to connect to and to verify (no brackets around an IPv6 address).
    pub(crate) host: &'a str,
    pub(crate) port: u16,
    /// `host[:port]` as the URL wrote it, for the `Host` header.
    pub(crate) authority: &'a str,
    /// `/path?query`.
    pub(crate) path: String,
}

// fuzz:parser fuzz_webhook_url
/// The parts of `url`, or `None` when it is not an `http://` or `https://` URL with a host and (if given) a numeric port.
pub(crate) fn split(url: &str) -> Option<Target<'_>> {
    let (tls, rest) = if let Some(r) = url.strip_prefix("https://") {
        (true, r)
    } else {
        (false, url.strip_prefix("http://")?)
    };
    let (authority, path) = rest
        .find(['/', '?', '#'])
        .map_or((rest, "/"), |i| rest.split_at(i));
    let path = path.split('#').next().unwrap_or_default();
    let path = if path.starts_with('?') || path.is_empty() {
        format!("/{path}")
    } else {
        path.to_owned()
    };
    // Credentials in the URL are not sent (AWS refuses such endpoints when they are made); only the host is kept.
    let hostport = authority.rsplit('@').next().unwrap_or(authority);
    let default = if tls { 443 } else { 80 };
    let (host, port) = if let Some(v6) = hostport.strip_prefix('[') {
        let (h, after) = v6.split_once(']')?;
        match after.strip_prefix(':') {
            Some(p) => (h, p.parse().ok()?),
            None if after.is_empty() => (h, default),
            None => return None,
        }
    } else {
        match hostport.rsplit_once(':') {
            Some((h, p)) => (h, p.parse().ok()?),
            None => (hostport, default),
        }
    };
    (!host.is_empty()).then_some(Target {
        tls,
        host,
        port,
        authority: hostport,
        path,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts(url: &str) -> Option<(bool, &str, u16, String)> {
        split(url).map(|t| (t.tls, t.host, t.port, t.path))
    }

    #[test]
    fn urls_are_taken_apart() {
        assert_eq!(
            parts("http://localhost:9000/a/b?c=1"),
            Some((false, "localhost", 9000, "/a/b?c=1".to_owned()))
        );
        assert_eq!(
            parts("http://example.test"),
            Some((false, "example.test", 80, "/".to_owned()))
        );
        assert_eq!(
            parts("https://example.test/x#frag"),
            Some((true, "example.test", 443, "/x".to_owned()))
        );
        assert_eq!(
            parts("https://[::1]:8443?q"),
            Some((true, "::1", 8443, "/?q".to_owned()))
        );
        assert_eq!(
            parts("http://h?x=1"),
            Some((false, "h", 80, "/?x=1".to_owned()))
        );
        assert_eq!(split("http://h:81/").map(|t| t.authority), Some("h:81"));
        assert_eq!(split("ftp://example.test/"), None);
        assert_eq!(split("http://:80/"), None);
        assert_eq!(split("http://h:notaport/"), None);
        assert_eq!(split("https://[::1/"), None);
    }
}
