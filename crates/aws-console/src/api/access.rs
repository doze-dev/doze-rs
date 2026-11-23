//! The Access space: who each caller is, and what IAM refused or would have refused.
//!
//! - `GET /api/v1/principals`: every caller in the Live record, named as IAM knows it (Live shows the name, the key on hover).
//! - `GET /api/v1/access`: the refused and would-be-refused calls of the Live record, by caller and operation, each with the
//!   rows of IAM's access log that say why; and every refusal in that log.
//! - `POST /api/v1/access/attach`: puts an inline policy on a user or a role (`PutUserPolicy`, `PutRolePolicy`) through the
//!   bus as the console, the "make this pass" of a refusal. Behind the same-origin proof, as every write.
//!
//! Names are resolved when asked, from IAM's own state: nothing is added to the request path or kept.

use std::collections::HashMap;

use axum::body::Bytes;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;
use serde_json::{Value, json};

use super::calls::fill;
use super::resources::refused;
use super::{error, json};
use crate::iam;
use crate::model::{AccessView, Attached, DenialGroup, Outcome, PrincipalsResponse};
use crate::ring::Call;
use crate::state::Shared;

/// Every call the ring holds.
const ALL: usize = usize::MAX;
/// The most resources a group lists.
const MOST_RESOURCES: usize = 5;
/// The longest policy document IAM takes for an inline user policy (2,048) is shorter than a role's (10,240): the larger is
/// the bound here, and IAM says when a user's is over its own.
const MOST_DOCUMENT: usize = 10_240;

/// `GET /api/v1/principals`.
pub async fn principals(State(s): State<Shared>) -> Response {
    let calls = s.recorder.newest(ALL).calls;
    let named = iam::principals(&s.facts, calls.iter().map(|c| &*c.caller)).await;
    let mut principals: Vec<_> = named.into_values().collect();
    principals.sort_by(|a, b| a.key.cmp(&b.key));
    json(StatusCode::OK, &PrincipalsResponse { principals })
}

/// Whether IAM refused `call`, or would have.
fn refused_call(c: &Call) -> bool {
    c.would_be_denied || Outcome::of(c.status, &c.code) == Outcome::Denied
}

/// The refused calls by (caller, service, operation), newest group first.
fn grouped(calls: &[Call], s: &Shared) -> Vec<(DenialGroup, &'static str)> {
    let mut groups: Vec<(DenialGroup, &'static str)> = Vec::new();
    let mut at: HashMap<(&str, &str, &str), usize> = HashMap::new();
    // Newest first, so each group's first call is its newest.
    for c in calls.iter().rev().filter(|c| refused_call(c)) {
        let key = (&*c.caller, c.signing_name, c.action);
        let i = *at.entry(key).or_insert_with(|| {
            let some = |t: &str| (!t.is_empty()).then(|| t.to_owned());
            groups.push((
                DenialGroup {
                    caller: c.caller.to_string(),
                    principal: iam::principal_of(&HashMap::new(), &c.caller),
                    service: some(s.facts.service_named(c.signing_name)),
                    api: some(c.sdk_id),
                    action: c.action.to_owned(),
                    denied: 0,
                    would_be_denied: 0,
                    first_ms: c.at_ms,
                    last_ms: c.at_ms,
                    last_seq: c.seq,
                    resources: Vec::new(),
                    explained: Vec::new(),
                    by_service: false,
                },
                c.signing_name,
            ));
            groups.len() - 1
        });
        let Some((g, _)) = groups.get_mut(i) else {
            continue;
        };
        if c.would_be_denied {
            g.would_be_denied += 1;
        } else {
            g.denied += 1;
        }
        g.first_ms = c.at_ms;
        if !c.resource.is_empty()
            && g.resources.len() < MOST_RESOURCES
            && !g.resources.iter().any(|r| **r == *c.resource)
        {
            g.resources.push(c.resource.to_string());
        }
    }
    groups
}

/// `GET /api/v1/access`.
pub async fn access(State(s): State<Shared>) -> Response {
    let calls = s.recorder.newest(ALL).calls;
    let found = grouped(&calls, &s);
    let named = iam::principals(&s.facts, found.iter().map(|(g, _)| g.caller.as_str())).await;
    let log = iam::refusals(&s.facts).await;
    let (refusals, evicted) = log.clone().unwrap_or_default();
    let groups: Vec<DenialGroup> = found
        .into_iter()
        .map(|(mut g, prefix)| {
            g.principal = iam::principal_of(&named, &g.caller);
            (g.explained, g.by_service) =
                iam::explaining(&refusals, &g.principal, &g.action, prefix);
            g
        })
        .collect();
    json(
        StatusCode::OK,
        &AccessView {
            iam: s.facts.iam.clone(),
            groups,
            refusals,
            evicted,
            log_read: log.is_some(),
            ring: fill(&s),
        },
    )
}

/// What an attach asks for, read from its JSON body.
fn attach_input(body: &[u8]) -> Result<(String, String, String, String), &'static str> {
    let Ok(Value::Object(v)) = serde_json::from_slice::<Value>(body) else {
        return Err("An attach is a JSON object: {kind, name, policy_name, document}.");
    };
    let text = |k: &str| {
        v.get(k)
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or_default()
            .to_owned()
    };
    let (kind, name, policy, document) = (
        text("kind"),
        text("name"),
        text("policy_name"),
        text("document"),
    );
    if kind != "user" && kind != "role" {
        return Err("kind is user or role.");
    }
    if name.is_empty() || policy.is_empty() || document.is_empty() {
        return Err("name, policy_name and document are each needed.");
    }
    if document.len() > MOST_DOCUMENT {
        return Err("The policy document is longer than IAM takes for an inline policy.");
    }
    if serde_json::from_str::<Value>(&document).is_err() {
        return Err("The policy document is not JSON.");
    }
    Ok((kind, name, policy, document))
}

/// `POST /api/v1/access/attach`.
pub async fn attach(State(s): State<Shared>, body: Bytes) -> Response {
    let (kind, name, policy_name, document) = match attach_input(&body) {
        Ok(input) => input,
        Err(why) => return error(StatusCode::BAD_REQUEST, "BAD_REQUEST", why),
    };
    let Some(bus) = s.bus() else {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "NO_BUS",
            "This console was started without the services' bus, so it cannot change IAM.",
        );
    };
    let (operation, member) = if kind == "user" {
        ("PutUserPolicy", "UserName")
    } else {
        ("PutRolePolicy", "RoleName")
    };
    let input = json!({member: name, "PolicyName": policy_name, "PolicyDocument": document});
    match bus.call("iam", operation, &input).await {
        Ok(_) => json(
            StatusCode::OK,
            &Attached {
                kind,
                name,
                policy_name,
            },
        ),
        Err(e) => refused(&e),
    }
}

#[cfg(test)]
mod tests {
    use super::attach_input;

    #[test]
    fn an_attach_names_a_user_or_role_a_policy_and_a_json_document() {
        let ok = br#"{"kind":"role","name":"worker","policy_name":"doze-drafted","document":"{\"Version\":\"2012-10-17\"}"}"#;
        assert_eq!(attach_input(ok).map(|t| t.0), Ok("role".to_owned()));
        for bad in [
            &br#"[]"#[..],
            br#"{"kind":"group","name":"g","policy_name":"p","document":"{}"}"#,
            br#"{"kind":"user","name":"","policy_name":"p","document":"{}"}"#,
            br#"{"kind":"user","name":"u","policy_name":"p","document":"not json"}"#,
        ] {
            assert!(attach_input(bad).is_err());
        }
    }
}
