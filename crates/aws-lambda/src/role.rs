//! Running as the function's role. A function's process gets credentials for its execution role: an STS session that IAM resolves,
//! so under `--iam enforce` it has the role's permissions and no others, and under `--iam soft` a call it makes that the role
//! does not allow is logged. The session is made with `AssumeRole` over the peer bus (Lambda is the service acting, so the trust
//! policy is not asked), kept until shortly before it ends, and made again for the next process.
//!
//! Whether AWS would accept the role is checked when a function is created or re-pointed: the role must exist and its trust policy
//! must let `lambda.amazonaws.com` assume it. A role IAM has never heard of is accepted unless IAM is enforcing, because a local
//! account is often created without the roles a template names; the function then runs as the local test identity.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

use aws_peers::{Mode, Peers};
use aws_wire::service::Caller;
use serde_json::{Value, json};

use crate::rt::spec::Creds;

/// The sessions made so far, by role and function.
#[derive(Debug, Default)]
pub(crate) struct Sessions {
    made: Mutex<HashMap<String, Creds>>,
}

/// The role name of `arn:aws:iam::<account>:role/<path>/<name>`.
fn role_name(arn: &str) -> Option<&str> {
    arn.split_once(":role/")
        .map(|(_, rest)| rest.rsplit('/').next().unwrap_or(rest))
}

/// Whether a trust policy document lets the Lambda service assume the role.
pub(crate) fn trusts_lambda(document: &str) -> bool {
    let decoded = aws_core::uri::decode_str(document)
        .map_or_else(|| document.to_owned(), std::borrow::Cow::into_owned);
    let Ok(doc) = serde_json::from_str::<Value>(&decoded) else {
        return false;
    };
    let statements = match doc.get("Statement") {
        Some(Value::Array(a)) => a.clone(),
        Some(one @ Value::Object(_)) => vec![one.clone()],
        _ => return false,
    };
    statements.iter().any(|s| {
        s.get("Effect").and_then(Value::as_str) == Some("Allow")
            && match s.get("Principal") {
                Some(Value::String(p)) => p == "*",
                Some(p) => {
                    mentions(p.get("Service"), "lambda.amazonaws.com")
                        || mentions(p.get("AWS"), "*")
                }
                None => false,
            }
    })
}

fn mentions(v: Option<&Value>, want: &str) -> bool {
    match v {
        Some(Value::String(s)) => s == want,
        Some(Value::Array(a)) => a.iter().any(|x| x.as_str() == Some(want)),
        _ => false,
    }
}

/// `Some(true)` when the role exists and trusts Lambda, `Some(false)` when AWS would refuse it, `None` when IAM cannot say.
pub(crate) async fn check(peers: Option<&Peers>, caller: &Caller, role: &str) -> Option<bool> {
    let peers = peers.filter(|p| p.mode() != Mode::Off)?;
    let name = role_name(role)?;
    let acting = Caller {
        service: Some("lambda".to_owned()),
        ..caller.clone()
    };
    match peers
        .call(
            "lambda",
            &acting,
            "iam",
            "GetRole",
            &json!({"RoleName": name}),
        )
        .await
    {
        Ok(got) => {
            let doc = got
                .pointer("/Role/AssumeRolePolicyDocument")
                .and_then(Value::as_str)?;
            Some(trusts_lambda(doc) || peers.mode() != Mode::Enforce)
        }
        Err(e) if e.code.starts_with("NoSuchEntity") => (peers.mode() == Mode::Enforce).then_some(false),
        Err(_) => None,
    }
}

impl Sessions {
    /// Credentials for a process of function `name` that runs as `role`: a session, or the local test identity when there is no
    /// role to assume or nothing to assume it.
    pub(crate) async fn credentials(
        &self,
        peers: Option<&Peers>,
        caller: &Caller,
        role: &str,
        name: &str,
        now_ms: i64,
    ) -> Creds {
        let key = format!("{role}/{name}");
        if let Some(c) = self
            .made
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&key)
            && c.expires_ms - now_ms > 600_000
        {
            return c.clone();
        }
        let Some(peers) = peers.filter(|p| p.mode() != Mode::Off) else {
            return crate::rt::test_credentials();
        };
        let acting = Caller {
            service: Some("lambda".to_owned()),
            ..caller.clone()
        };
        let ask = json!({"RoleArn": role, "RoleSessionName": format!("awslambda_{name}_{now_ms}").chars().take(64).collect::<String>(), "DurationSeconds": 3600});
        let got = match peers
            .call("lambda", &acting, "sts", "AssumeRole", &ask)
            .await
        {
            Ok(got) => got,
            Err(e) => {
                tracing::warn!("could not assume {role} for {name}: {}", e.message);
                return crate::rt::test_credentials();
            }
        };
        let Some(c) = got.get("Credentials") else {
            tracing::warn!("{role} was assumed and no credentials came back");
            return crate::rt::test_credentials();
        };
        let text = |k: &str| {
            c.get(k)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        let expires_ms = match c.get("Expiration") {
            Some(Value::Number(n)) => n.as_f64().map_or(0, |s| (s * 1000.0) as i64),
            Some(Value::String(s)) => aws_core::time::parse_rfc3339(s).map_or(0, |s| s * 1000),
            _ => 0,
        };
        let made = Creds {
            key: text("AccessKeyId"),
            secret: text("SecretAccessKey"),
            token: text("SessionToken"),
            expires_ms,
        };
        self.made
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(key, made.clone());
        made
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_trust_policy_must_name_lambda() {
        let lambda = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":{"Service":"lambda.amazonaws.com"},"Action":"sts:AssumeRole"}]}"#;
        let ec2 = lambda.replace("lambda", "ec2");
        assert!(trusts_lambda(lambda));
        assert!(!trusts_lambda(&ec2));
        assert!(trusts_lambda(&aws_core::uri::encode(
            lambda.as_bytes(),
            false
        )));
        assert_eq!(role_name("arn:aws:iam::1:role/service-role/r"), Some("r"));
    }
}
