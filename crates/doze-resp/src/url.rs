//! The `REDIS_URL` doze hands an app.

/// `redis://[:password@]addr/0`. The password is percent-encoded: the Go modules put it in as written, so a password
/// holding `@`, `:` or `/` gave a URL that named another host (module bug 7, docs/port/modules/PLAN.md). An empty
/// password is no password.
pub fn redis_url(password: Option<&str>, addr: &str) -> String {
    match password.filter(|p| !p.is_empty()) {
        Some(p) => format!("redis://:{}@{addr}/0", escape(p)),
        None => format!("redis://{addr}/0"),
    }
}

/// Percent-encodes everything but RFC 3986's unreserved characters.
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_password_cannot_change_where_the_url_points() {
        assert_eq!(
            redis_url(None, "127.0.0.1:6379"),
            "redis://127.0.0.1:6379/0"
        );
        assert_eq!(redis_url(Some(""), "h:1"), "redis://h:1/0");
        assert_eq!(
            redis_url(Some("s3cret-1"), "h:1"),
            "redis://:s3cret-1@h:1/0"
        );
        assert_eq!(
            redis_url(Some("a@evil:9/x é"), "h:1"),
            "redis://:a%40evil%3A9%2Fx%20%C3%A9@h:1/0"
        );
    }
}
