//! Policy variables: `${aws:username}` in a resource or in a condition value stands for the request's own value of that key.

use super::context::Context;

/// `s` with its variables replaced. `None` when one names a key the request does not have: such a value matches nothing.
/// `${*}`, `${?}` and `${$}` are a literal `*`, `?` and `$`.
#[must_use]
pub fn substitute(s: &str, ctx: &Context) -> Option<String> {
    if !s.contains("${") {
        return Some(s.to_owned());
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(at) = rest.find("${") {
        out.push_str(rest.get(..at)?);
        let after = rest.get(at + 2..)?;
        let end = after.find('}')?;
        let name = after.get(..end)?;
        match name {
            "*" | "?" | "$" => out.push_str(name),
            key => out.push_str(ctx.get(key)?.first()?),
        }
        rest = after.get(end + 1..)?;
    }
    out.push_str(rest);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variables_take_the_requests_value_or_match_nothing() {
        let mut c = Context::new();
        c.set("aws:username", "alice");
        assert_eq!(
            substitute("arn:aws:s3:::b/${aws:username}/*", &c).as_deref(),
            Some("arn:aws:s3:::b/alice/*")
        );
        assert_eq!(substitute("plain", &c).as_deref(), Some("plain"));
        assert_eq!(substitute("${aws:userid}", &c), None);
        assert_eq!(substitute("a${$}b${*}", &c).as_deref(), Some("a$b*"));
        assert_eq!(substitute("${unclosed", &c), None);
    }
}
