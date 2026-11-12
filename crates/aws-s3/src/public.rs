//! Whether a bucket policy is public: the question behind `GetBucketPolicyStatus` and the `BlockPublicPolicy` refusal.
//!
//! AWS's rule (S3 User Guide, "The meaning of public", read 2026-11-13): a policy is assumed public, and is not public only when every
//! statement that allows grants to fixed values of a principal or of one of these condition keys: `aws:SourceIp` (a CIDR no broader
//! than /8 for IPv4 or /32 for IPv6), `aws:SourceArn`, `aws:SourceVpc`, `aws:SourceVpce`, `aws:SourceOwner`, `aws:SourceAccount`,
//! `aws:userid` (outside the pattern `AROLEID:*`), `s3:DataAccessPointArn` (whose access point name may be a wildcard while the account
//! is fixed) and `s3:DataAccessPointAccount`; the principal keys (`aws:PrincipalArn`, `aws:PrincipalAccount`, `aws:PrincipalOrgID`,
//! `aws:PrincipalOrgPaths`) count as naming a principal. A fixed value holds no wildcard (`*`, `?`) and no policy variable (`${…}`). A
//! condition on anything else (a date, `aws:SecureTransport`, `s3:prefix`) restricts *when*, not *who*, and leaves the grant public.
//!
//! What AWS does not document, and is taken from the Go edition's engine (memory): a negated operator (`StringNotEquals`) and an
//! `…IfExists` operator do not narrow (they let through callers without the key); an explicit `Deny` cancels a public grant only when it
//! is unconditional, names everyone with `Principal`, and covers the grant's actions and resources; a `Deny` with `NotAction`,
//! `NotResource` or `NotPrincipal` is not weighed. Where the rule is uncertain the answer is "public", the safe side for a setting whose
//! job is catching an exposed bucket.

use std::net::IpAddr;

use serde_json::{Map, Value};

/// The condition keys whose fixed values make a grant non-public (lower case).
const NARROWING: &[&str] = &[
    "aws:sourceip",
    "aws:sourcearn",
    "aws:sourcevpc",
    "aws:sourcevpce",
    "aws:sourceowner",
    "aws:sourceaccount",
    "aws:userid",
    "s3:dataaccesspointarn",
    "s3:dataaccesspointaccount",
    "aws:principalarn",
    "aws:principalaccount",
    "aws:principalorgid",
    "aws:principalorgpaths",
];

/// Whether a policy document makes its bucket public.
pub(crate) fn is_public(doc: &Value) -> bool {
    let statements = statements(doc);
    statements.iter().any(|s| {
        let Some(st) = s.as_object() else {
            return false;
        };
        is_allow(st) && grants_everyone(st) && !condition_narrows(st.get("Condition")) && {
            let actions = probes(st, "Action", "NotAction");
            let resources = probes(st, "Resource", "NotResource");
            actions.iter().any(|a| {
                resources
                    .iter()
                    .any(|r| !cancelled_by_deny(&statements, a, r))
            })
        }
    })
}

fn statements(doc: &Value) -> Vec<Value> {
    match doc.get("Statement") {
        Some(Value::Array(a)) => a.clone(),
        Some(s @ Value::Object(_)) => vec![s.clone()],
        _ => Vec::new(),
    }
}

fn effect(st: &Map<String, Value>) -> &str {
    st.get("Effect").and_then(Value::as_str).unwrap_or_default()
}

fn is_allow(st: &Map<String, Value>) -> bool {
    effect(st).eq_ignore_ascii_case("Allow")
}

/// A string or a list of strings as a list.
fn strings(v: Option<&Value>) -> Vec<String> {
    match v {
        Some(Value::String(s)) => vec![s.clone()],
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(|x| x.as_str().map(str::to_owned))
            .collect(),
        _ => Vec::new(),
    }
}

/// Whether a value is fixed: no wildcard and no policy variable.
fn fixed(v: &str) -> bool {
    !v.contains(['*', '?']) && !v.contains("${")
}

/// `Principal: "*"`, `{"AWS": "*"}`, an AWS principal that is not a fixed value (`arn:aws:iam::*:root`), or an allowing
/// `NotPrincipal` (which grants everyone it does not name).
fn grants_everyone(st: &Map<String, Value>) -> bool {
    if st.contains_key("NotPrincipal") {
        return true;
    }
    match st.get("Principal") {
        Some(Value::String(s)) => !fixed(s),
        Some(Value::Object(m)) => m
            .values()
            .any(|v| strings(Some(v)).iter().any(|p| !fixed(p))),
        _ => false,
    }
}

/// Whether a condition block limits who the grant reaches: a non-negated operator on a narrowing key with only fixed values.
fn condition_narrows(cond: Option<&Value>) -> bool {
    let Some(Value::Object(ops)) = cond else {
        return false;
    };
    ops.iter().any(|(op, keys)| {
        let base = op
            .split_once(':')
            .map_or(op.as_str(), |(_, rest)| rest)
            .to_ascii_lowercase();
        if base.contains("not") || base.ends_with("ifexists") || base == "null" {
            return false;
        }
        keys.as_object().is_some_and(|keys| {
            keys.iter().any(|(key, values)| {
                let key = key.to_ascii_lowercase();
                NARROWING.contains(&key.as_str()) && {
                    let values = strings(Some(values));
                    !values.is_empty() && values.iter().all(|v| fixed_for(&key, v))
                }
            })
        })
    })
}

/// Whether `v` is a fixed value for `key`, with the two exceptions AWS documents.
fn fixed_for(key: &str, v: &str) -> bool {
    match key {
        "aws:sourceip" => narrow_cidr(v),
        // `AROLEID:*`: one role's sessions.
        "aws:userid" => v
            .strip_suffix(":*")
            .map_or_else(|| fixed(v), |id| !id.is_empty() && fixed(id)),
        // An access point ARN may leave the name open while its account is fixed.
        "s3:dataaccesspointarn" => {
            let parts: Vec<&str> = v.splitn(6, ':').collect();
            match parts.as_slice() {
                [_, _, _, region, account, resource] => {
                    fixed(region)
                        && fixed(account)
                        && !account.is_empty()
                        && resource.starts_with("accesspoint/")
                }
                _ => false,
            }
        }
        _ => fixed(v),
    }
}

/// A CIDR (or one address) no broader than /8 for IPv4 or /32 for IPv6; the private ranges of RFC 1918 are always narrow.
fn narrow_cidr(v: &str) -> bool {
    let (addr, bits) = match v.split_once('/') {
        Some((a, b)) => (a, b.parse::<u8>().ok()),
        None => (v, None),
    };
    let Ok(ip) = addr.parse::<IpAddr>() else {
        return false;
    };
    match ip {
        IpAddr::V4(v4) => bits.unwrap_or(32) >= 8 || v4.is_private(),
        IpAddr::V6(_) => bits.unwrap_or(128) >= 32,
    }
}

/// The patterns a statement grants over: its list, or `*` for a negated list or none (a resource policy's statement with no
/// `Resource` applies to the resource it is on).
fn probes(st: &Map<String, Value>, member: &str, negated: &str) -> Vec<String> {
    let list = strings(st.get(member));
    if list.is_empty() || st.contains_key(negated) {
        vec!["*".to_owned()]
    } else {
        list
    }
}

/// Whether an unconditional `Deny` to everyone covers the granted action and resource.
fn cancelled_by_deny(statements: &[Value], action: &str, resource: &str) -> bool {
    statements.iter().filter_map(Value::as_object).any(|st| {
        let everyone = matches!(st.get("Principal"), Some(Value::String(p)) if p == "*")
            || strings(st.get("Principal").and_then(|p| p.get("AWS")))
                .iter()
                .any(|p| p == "*");
        effect(st).eq_ignore_ascii_case("Deny")
            && everyone
            && ["Condition", "NotPrincipal", "NotAction", "NotResource"]
                .iter()
                .all(|k| !st.contains_key(*k))
            && strings(st.get("Action"))
                .iter()
                .any(|p| glob(&p.to_ascii_lowercase(), &action.to_ascii_lowercase()))
            && strings(st.get("Resource"))
                .iter()
                .any(|p| glob(p, resource))
    })
}

/// IAM's wildcard match: `*` is any run of characters, `?` any one.
fn glob(pattern: &str, text: &str) -> bool {
    let (p, t): (Vec<char>, Vec<char>) = (pattern.chars().collect(), text.chars().collect());
    let (mut pi, mut ti) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while ti < t.len() {
        match p.get(pi) {
            Some('*') => {
                star = Some((pi, ti));
                pi += 1;
            }
            Some(c) if *c == '?' || Some(c) == t.get(ti) => {
                pi += 1;
                ti += 1;
            }
            _ => match star {
                Some((sp, st)) => {
                    pi = sp + 1;
                    ti = st + 1;
                    star = Some((sp, st + 1));
                }
                None => return false,
            },
        }
    }
    p.get(pi..)
        .is_some_and(|rest| rest.iter().all(|c| *c == '*'))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn one(statement: &Value) -> bool {
        is_public(&json!({"Version": "2012-10-17", "Statement": [statement]}))
    }

    fn allow(principal: &Value, condition: Option<Value>) -> Value {
        let mut s = json!({"Effect": "Allow", "Principal": principal, "Action": "s3:GetObject", "Resource": "arn:aws:s3:::b/*"});
        if let Some(c) = condition {
            s["Condition"] = c;
        }
        s
    }

    #[test]
    fn the_documented_examples() {
        // The three examples of the user guide.
        let star = json!("*");
        assert!(one(&allow(&star, None)));
        assert!(one(&allow(
            &star,
            Some(json!({"StringLike": {"aws:SourceVpc": "vpc-*"}}))
        )));
        assert!(!one(&allow(
            &star,
            Some(json!({"StringEquals": {"aws:SourceVpc": "vpc-91237329"}}))
        )));
    }

    #[test]
    fn a_condition_on_when_and_not_who_leaves_it_public() {
        let star = json!({"AWS": "*"});
        assert!(one(&allow(
            &star,
            Some(json!({"Bool": {"aws:SecureTransport": "true"}}))
        )));
        assert!(one(&allow(
            &star,
            Some(json!({"StringLike": {"s3:prefix": "home/"}}))
        )));
        assert!(!one(&allow(
            &star,
            Some(json!({"StringEquals": {"aws:PrincipalOrgID": "o-abc"}}))
        )));
        assert!(
            one(&allow(
                &star,
                Some(json!({"StringNotEquals": {"aws:SourceAccount": "111122223333"}}))
            )),
            "a negated operator widens"
        );
        assert!(one(&allow(
            &star,
            Some(json!({"StringEqualsIfExists": {"aws:SourceArn": "arn:aws:sns:us-east-1:1:t"}}))
        )));
        assert!(one(&allow(
            &star,
            Some(json!({"ArnLike": {"aws:SourceArn": "${aws:username}"}}))
        )));
    }

    #[test]
    fn source_ip_ranges_broader_than_a_slash_eight_are_public() {
        let star = json!("*");
        let ip = |cidr: &str| allow(&star, Some(json!({"IpAddress": {"aws:SourceIp": cidr}})));
        assert!(!one(&ip("203.0.113.0/24")));
        assert!(!one(&ip("10.0.0.0/8")));
        assert!(one(&ip("0.0.0.0/1")));
        assert!(one(&ip("0.0.0.0/0")));
        assert!(!one(&ip("2001:db8::/32")));
        assert!(one(&ip("2001:db8::/16")));
    }

    #[test]
    fn userid_and_access_points_have_their_exceptions() {
        let star = json!("*");
        let c = |k: &str, v: &str| allow(&star, Some(json!({"StringLike": {k: v}})));
        assert!(!one(&c("aws:userid", "AROAEXAMPLEID:*")));
        assert!(one(&c("aws:userid", "*")));
        assert!(!one(&c(
            "s3:DataAccessPointArn",
            "arn:aws:s3:us-west-2:123456789012:accesspoint/*"
        )));
        assert!(one(&c(
            "s3:DataAccessPointArn",
            "arn:aws:s3:us-west-2:*:accesspoint/*"
        )));
    }

    #[test]
    fn a_named_principal_is_not_public_and_a_wildcard_one_is() {
        assert!(!one(&allow(
            &json!({"AWS": "arn:aws:iam::111122223333:root"}),
            None
        )));
        assert!(!one(&allow(
            &json!({"Service": "cloudtrail.amazonaws.com"}),
            None
        )));
        assert!(one(&allow(&json!({"AWS": "arn:aws:iam::*:root"}), None)));
        let not_principal = json!({"Effect": "Allow", "NotPrincipal": {"AWS": "arn:aws:iam::1:root"}, "Action": "s3:*", "Resource": "*"});
        assert!(one(&not_principal));
    }

    #[test]
    fn a_broad_unconditional_deny_to_everyone_cancels_the_grant() {
        let grant = allow(&json!("*"), None);
        let deny = |action: &str, cond: Option<Value>| {
            let mut d = json!({"Effect": "Deny", "Principal": "*", "Action": action, "Resource": "arn:aws:s3:::b/*"});
            if let Some(c) = cond {
                d["Condition"] = c;
            }
            d
        };
        let doc = |d: Value| json!({"Statement": [grant.clone(), d]});
        assert!(!is_public(&doc(deny("s3:*", None))));
        assert!(!is_public(&doc(deny("s3:GetObject", None))));
        assert!(is_public(&doc(deny("s3:PutObject", None))));
        assert!(is_public(&doc(deny(
            "s3:*",
            Some(json!({"Bool": {"aws:SecureTransport": "false"}}))
        ))));
        // One statement as an object, not a list.
        assert!(is_public(&json!({"Statement": allow(&json!("*"), None)})));
        assert!(!is_public(&json!({"Statement": []})));
    }
}
