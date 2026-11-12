//! The same-origin proof every request that is not a plain read must carry.
//!
//! The console has no login, so the browser is the only thing that says who is asking, and a web page in another tab
//! can make the browser post a form to `http://127.0.0.1:9093/...` with the right `Host` and no preflight. What it
//! cannot do is make the browser lie about where the request came from: `Sec-Fetch-Site` and `Origin` are set by
//! the browser itself. So a request that changes something is served only if the browser says it is from this origin.
//!
//! The rules, strict because a refusal costs a header and an acceptance costs the defence:
//! - `GET` and `HEAD` pass: they change nothing here (no route that changes anything answers them).
//! - `Sec-Fetch-Site: same-origin` passes. `same-site`, `cross-site` and `none` do not.
//! - With no `Sec-Fetch-Site` the `Origin` must be `http://` or `https://` and an authority equal, case apart, to the
//!   `Host` the request was sent to. `null`, another scheme, a path, userinfo, a different host or port: refused.
//! - With neither header, or no `Host`, it is refused: a browser always sends one of them on a post.

/// Why a request was refused. The words are shown to the person who sees the 403.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The browser says the request came from another site.
    CrossSite,
    /// No `Origin` and no `Sec-Fetch-Site`: nothing says where it came from.
    NoProof,
    /// An `Origin` that is not an origin (`null`, another scheme, userinfo, a path).
    BadOrigin,
    /// An `Origin` that names another host or port than the one asked.
    OtherOrigin,
    /// No `Host` to compare with.
    NoHost,
}

impl Refusal {
    /// What to tell the person.
    #[must_use]
    pub const fn words(self) -> &'static str {
        match self {
            Self::CrossSite => {
                "The browser says this request was sent by another site, and the console only accepts changes made from its own pages."
            }
            Self::NoProof => {
                "The request carries neither an Origin nor a Sec-Fetch-Site header, so nothing says it was made from the console's own pages. Use the console in a browser, or send an Origin header that equals the Host."
            }
            Self::BadOrigin => "The Origin header is not an http or https origin.",
            Self::OtherOrigin => {
                "The Origin header names another host or port than the one this request was sent to."
            }
            Self::NoHost => {
                "The request has no Host header, so its Origin cannot be compared with anything."
            }
        }
    }
}

// fuzz:parser origin
/// Whether a request may be served: `method` as sent, and the three headers as the browser sent them (`None` when
/// absent). Pure: the same answer for the same text.
///
/// # Errors
/// The [`Refusal`] that says why not.
pub fn check(
    method: &str,
    origin: Option<&str>,
    fetch_site: Option<&str>,
    host: Option<&str>,
) -> Result<(), Refusal> {
    if method.eq_ignore_ascii_case("GET") || method.eq_ignore_ascii_case("HEAD") {
        return Ok(());
    }
    match fetch_site.map(str::trim) {
        Some(s) if s.eq_ignore_ascii_case("same-origin") => return Ok(()),
        Some(s) if !s.is_empty() => return Err(Refusal::CrossSite),
        _ => {}
    }
    let Some(origin) = origin else {
        return Err(Refusal::NoProof);
    };
    let Some(authority) = authority_of(origin.trim()) else {
        return Err(Refusal::BadOrigin);
    };
    let Some(host) = host.map(str::trim).filter(|h| !h.is_empty()) else {
        return Err(Refusal::NoHost);
    };
    if authority.eq_ignore_ascii_case(host) {
        Ok(())
    } else {
        Err(Refusal::OtherOrigin)
    }
}

/// The authority of an origin written `http://host[:port]` or `https://host[:port]`, and nothing else: no path, no
/// userinfo, no query, no spaces.
fn authority_of(origin: &str) -> Option<&str> {
    let rest = origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"))?;
    let clean = !rest.is_empty()
        && rest.bytes().all(|b| {
            b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b':' | b'[' | b']')
        });
    clean.then_some(rest)
}

#[cfg(test)]
mod tests;
