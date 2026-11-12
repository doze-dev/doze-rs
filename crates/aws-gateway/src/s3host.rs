//! S3 addressing: which bucket a request is for when it is named in the host (`bucket.s3.amazonaws.com`) and not in the path.

use http::header;
use http::request::Parts;

/// The path to route (the bucket put back in front of a virtual-host path) and the host without its bucket label.
pub(crate) fn target(parts: &Parts) -> (String, String) {
    let host = parts
        .headers
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("localhost");
    let path = parts.uri.path();
    match bucket_of_host(host) {
        Some((bucket, rest)) => {
            let trimmed = path.trim_start_matches('/');
            (format!("/{bucket}/{trimmed}"), rest)
        }
        None => (path.to_owned(), host.to_owned()),
    }
}

/// `bucket.s3.amazonaws.com` or `bucket.s3.us-west-2.amazonaws.com:4566`: the bucket, and the host that remains. A host with no
/// `s3` label after at least one other label (and any IP address) is a path-style host.
fn bucket_of_host(host: &str) -> Option<(String, String)> {
    let name = host.rsplit_once(':').map_or(host, |(n, p)| {
        if p.bytes().all(|b| b.is_ascii_digit()) {
            n
        } else {
            host
        }
    });
    if name.parse::<std::net::IpAddr>().is_ok() || name.starts_with('[') {
        return None;
    }
    let labels: Vec<&str> = name.split('.').collect();
    let at = labels
        .iter()
        .position(|l| *l == "s3" || l.starts_with("s3-"))
        .filter(|at| *at > 0)?;
    let bucket = labels.get(..at)?.join(".");
    let rest = host.get(bucket.len() + 1..)?.to_owned();
    Some((bucket, rest))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_virtual_host_names_the_bucket() {
        let cases = [
            ("b.s3.amazonaws.com", Some(("b", "s3.amazonaws.com"))),
            (
                "my.bucket.s3.us-west-2.amazonaws.com",
                Some(("my.bucket", "s3.us-west-2.amazonaws.com")),
            ),
            ("b.s3.localhost:4566", Some(("b", "s3.localhost:4566"))),
            (
                "b.s3-us-west-2.amazonaws.com",
                Some(("b", "s3-us-west-2.amazonaws.com")),
            ),
            ("s3.amazonaws.com", None),
            ("127.0.0.1:4566", None),
            ("localhost:4566", None),
            ("[::1]:4566", None),
        ];
        for (host, want) in cases {
            assert_eq!(
                bucket_of_host(host),
                want.map(|(b, r)| (b.to_owned(), r.to_owned())),
                "{host}"
            );
        }
    }
}
