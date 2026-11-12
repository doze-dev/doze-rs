//! CORS: how a bucket's `CORSConfiguration` answers a browser. A preflight (`OPTIONS` with `Origin` and
//! `Access-Control-Request-Method`) is matched against the rules in order and the first that admits the origin, the method and every
//! requested header answers it; one that no rule admits is refused with 403. An actual request that carries `Origin` gets the
//! `Access-Control-*` headers of the first rule that admits its origin and method, and is served either way.
//!
//! Matching follows the S3 User Guide ("How does Amazon S3 evaluate the CORS configuration on a bucket?"): an `AllowedOrigin` or
//! `AllowedHeader` may hold one `*` standing for any run of characters; header names are compared without case; methods exactly. The
//! headers of the answer, and the wording of the refusals, are from memory: `Access-Control-Allow-Origin` is `*` when the matching
//! rule allows `*` and the origin otherwise (with `Access-Control-Allow-Credentials: true`), `Access-Control-Allow-Methods` lists the
//! rule's methods, `Access-Control-Allow-Headers` echoes the requested headers, and `Vary` names what the answer depends on.

use aws_wire::ApiError;
use serde_json::Value;

/// The headers of an answer.
pub(crate) type Headers = Vec<(String, String)>;

/// One rule of a stored configuration.
#[derive(Debug, Default)]
struct Rule {
    origins: Vec<String>,
    methods: Vec<String>,
    headers: Vec<String>,
    expose: Vec<String>,
    max_age: Option<i64>,
}

fn list(v: &Value, member: &str) -> Vec<String> {
    v.get(member)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|x| x.as_str().map(str::to_owned))
        .collect()
}

/// The rules of a configuration as it is stored (the JSON of `PutBucketCors`'s `CORSConfiguration`).
fn rules(stored: &str) -> Vec<Rule> {
    let v: Value = serde_json::from_str(stored).unwrap_or(Value::Null);
    v.get("CORSRules")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|r| Rule {
            origins: list(r, "AllowedOrigins"),
            methods: list(r, "AllowedMethods"),
            headers: list(r, "AllowedHeaders"),
            expose: list(r, "ExposeHeaders"),
            max_age: r.get("MaxAgeSeconds").and_then(Value::as_i64),
        })
        .collect()
}

/// `pattern` with at most one `*` against `text`.
fn one_star(pattern: &str, text: &str) -> bool {
    match pattern.split_once('*') {
        Some((head, tail)) => {
            text.len() >= head.len() + tail.len() && text.starts_with(head) && text.ends_with(tail)
        }
        None => pattern == text,
    }
}

impl Rule {
    fn origin(&self, origin: &str) -> Option<String> {
        self.origins.iter().find(|p| one_star(p, origin)).map(|p| {
            if p == "*" {
                "*".to_owned()
            } else {
                origin.to_owned()
            }
        })
    }

    fn header(&self, name: &str) -> bool {
        let name = name.to_ascii_lowercase();
        self.headers
            .iter()
            .any(|p| one_star(&p.to_ascii_lowercase(), &name))
    }

    /// The headers every answer this rule makes carries.
    fn common(&self, allowed: String) -> Headers {
        let mut out = vec![("access-control-allow-origin".to_owned(), allowed.clone())];
        out.push((
            "access-control-allow-methods".to_owned(),
            self.methods.join(", "),
        ));
        if !self.expose.is_empty() {
            out.push((
                "access-control-expose-headers".to_owned(),
                self.expose.join(", "),
            ));
        }
        if let Some(age) = self.max_age {
            out.push(("access-control-max-age".to_owned(), age.to_string()));
        }
        if allowed != "*" {
            out.push((
                "access-control-allow-credentials".to_owned(),
                "true".to_owned(),
            ));
        }
        out
    }
}

/// What a preflight asks.
#[derive(Debug)]
pub(crate) struct Asked<'a> {
    pub(crate) origin: Option<&'a str>,
    pub(crate) method: Option<&'a str>,
    /// `Access-Control-Request-Headers`, as sent.
    pub(crate) headers: Option<&'a str>,
    /// Whether the path names an object (it says so in the refusal).
    pub(crate) object: bool,
}

/// The answer to a preflight: the headers of a 200, or the refusal.
///
/// # Errors
/// A preflight without an origin or a method (400), on a bucket with no configuration, or that no rule admits (403).
pub(crate) fn preflight(stored: Option<&str>, asked: &Asked<'_>) -> Result<Headers, ApiError> {
    // (memory) the wording of the three refusals.
    let origin = asked.origin.filter(|o| !o.is_empty()).ok_or_else(|| {
        ApiError::new(
            400,
            "BadRequest",
            "Insufficient information. Origin request header needed.",
        )
    })?;
    let method = asked.method.filter(|m| !m.is_empty()).ok_or_else(|| {
        ApiError::new(
            400,
            "BadRequest",
            "Invalid Access-Control-Request-Method: null",
        )
    })?;
    let resource = if asked.object { "OBJECT" } else { "BUCKET" };
    let forbidden = |message: &str| {
        ApiError::new(403, "AccessForbidden", message)
            .with("Method", method)
            .with("ResourceType", resource)
    };
    let Some(stored) = stored else {
        return Err(forbidden(
            "CORSResponse: CORS is not enabled for this bucket.",
        ));
    };
    let wanted: Vec<&str> = asked
        .headers
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|h| !h.is_empty())
        .collect();
    for rule in rules(stored) {
        let Some(allowed) = rule.origin(origin) else {
            continue;
        };
        if !rule.methods.iter().any(|m| m == method) || !wanted.iter().all(|h| rule.header(h)) {
            continue;
        }
        let mut out = rule.common(allowed);
        if !wanted.is_empty() {
            out.push((
                "access-control-allow-headers".to_owned(),
                wanted.join(", ").to_ascii_lowercase(),
            ));
        }
        out.push((
            "vary".to_owned(),
            "Origin, Access-Control-Request-Headers, Access-Control-Request-Method".to_owned(),
        ));
        return Ok(out);
    }
    Err(forbidden(
        "CORSResponse: This CORS request is not allowed. This is usually because the evalution of Origin, request method / Access-Control-Request-Method or Access-Control-Request-Headers are not whitelisted by the resource's CORS spec.",
    ))
}

/// The headers an actual request with `Origin` gets: those of the first rule that admits its origin and method, or none.
pub(crate) fn actual(stored: Option<&str>, origin: &str, method: &str) -> Headers {
    let Some(stored) = stored else {
        return Vec::new();
    };
    for rule in rules(stored) {
        if let Some(allowed) = rule.origin(origin)
            && rule.methods.iter().any(|m| m == method)
        {
            let mut out = rule.common(allowed);
            out.push((
                "vary".to_owned(),
                "Origin, Access-Control-Request-Headers, Access-Control-Request-Method".to_owned(),
            ));
            return out;
        }
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = r#"{"CORSRules":[
        {"AllowedOrigins":["https://*.example.com"],"AllowedMethods":["PUT","POST"],"AllowedHeaders":["content-*","x-amz-*"],
         "ExposeHeaders":["ETag"],"MaxAgeSeconds":3000},
        {"AllowedOrigins":["*"],"AllowedMethods":["GET"]}]}"#;

    fn ask<'a>(origin: &'a str, method: &'a str, headers: Option<&'a str>) -> Asked<'a> {
        Asked {
            origin: Some(origin),
            method: Some(method),
            headers,
            object: true,
        }
    }

    fn get<'a>(h: &'a Headers, name: &str) -> Option<&'a str> {
        h.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    #[test]
    fn the_first_rule_that_admits_origin_method_and_headers_answers() {
        let h = preflight(
            Some(CONFIG),
            &ask(
                "https://app.example.com",
                "PUT",
                Some("Content-Type, X-Amz-Date"),
            ),
        )
        .unwrap();
        assert_eq!(
            get(&h, "access-control-allow-origin"),
            Some("https://app.example.com")
        );
        assert_eq!(get(&h, "access-control-allow-methods"), Some("PUT, POST"));
        assert_eq!(
            get(&h, "access-control-allow-headers"),
            Some("content-type, x-amz-date")
        );
        assert_eq!(get(&h, "access-control-expose-headers"), Some("ETag"));
        assert_eq!(get(&h, "access-control-max-age"), Some("3000"));
        assert_eq!(get(&h, "access-control-allow-credentials"), Some("true"));
        let any = preflight(Some(CONFIG), &ask("http://other.test", "GET", None)).unwrap();
        assert_eq!(get(&any, "access-control-allow-origin"), Some("*"));
        assert_eq!(get(&any, "access-control-allow-credentials"), None);
    }

    #[test]
    fn a_preflight_no_rule_admits_is_refused() {
        let refused = |a: &Asked<'_>| preflight(Some(CONFIG), a).unwrap_err();
        let e = refused(&ask("http://other.test", "PUT", None));
        assert_eq!((e.status, e.code.as_ref()), (403, "AccessForbidden"));
        assert!(
            e.message
                .starts_with("CORSResponse: This CORS request is not allowed")
        );
        // A header the rule does not allow.
        refused(&ask(
            "https://app.example.com",
            "PUT",
            Some("authorization"),
        ));
        // A method no rule names.
        refused(&ask("https://app.example.com", "DELETE", None));
        // `*.example.com` needs something before the dot.
        refused(&ask("https://example.com", "PUT", None));
        let none = preflight(None, &ask("http://a", "GET", None)).unwrap_err();
        assert_eq!(
            none.message,
            "CORSResponse: CORS is not enabled for this bucket."
        );
        let no_origin = Asked {
            origin: None,
            method: Some("GET"),
            headers: None,
            object: false,
        };
        assert_eq!(preflight(Some(CONFIG), &no_origin).unwrap_err().status, 400);
    }

    #[test]
    fn an_actual_request_is_decorated_only_when_a_rule_admits_it() {
        let h = actual(Some(CONFIG), "https://app.example.com", "PUT");
        assert_eq!(
            get(&h, "access-control-allow-origin"),
            Some("https://app.example.com")
        );
        assert_eq!(get(&h, "access-control-expose-headers"), Some("ETag"));
        assert!(actual(Some(CONFIG), "https://app.example.com", "DELETE").is_empty());
        assert_eq!(
            get(
                &actual(Some(CONFIG), "http://x", "GET"),
                "access-control-allow-origin"
            ),
            Some("*")
        );
        assert!(actual(None, "http://x", "GET").is_empty());
    }
}
