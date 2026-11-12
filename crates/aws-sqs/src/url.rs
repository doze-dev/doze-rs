//! Queue URLs: `<endpoint>/<account>/<name>`.

/// The URL of a queue.
pub(crate) fn build(endpoint: &str, account: &str, name: &str) -> String {
    format!("{endpoint}/{account}/{name}")
}

/// The account and queue name a URL names, if it has the shape of one. The scheme and host are not checked: a queue is
/// reached by whatever address the caller used.
pub(crate) fn parse(url: &str) -> Option<(&str, &str)> {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let path = rest.split_once('/')?.1;
    let (account, name) = path.split_once('/')?;
    (!account.is_empty() && !name.is_empty() && !name.contains('/')).then_some((account, name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_url_round_trips_and_a_wrong_shape_does_not_parse() {
        let url = build("http://127.0.0.1:4566", "000000000000", "jobs");
        assert_eq!(parse(&url), Some(("000000000000", "jobs")));
        assert_eq!(
            parse("https://sqs.us-east-1.amazonaws.com/123456789012/q-1"),
            Some(("123456789012", "q-1"))
        );
        for bad in [
            "",
            "not a url",
            "http://host",
            "http://host/acct",
            "http://host/acct/",
            "http://host/a/b/c",
        ] {
            assert_eq!(parse(bad), None, "{bad}");
        }
    }
}
