//! `GET /api/v1/access/policies?kind=user&name=report-job`: the policies a user or a role holds, read through the bus as the
//! console, for Live's "Policies evaluated" and for "Add to policy" to edit the policy that is there instead of making another.
//!
//! The user's own inline policies and the managed policies attached to it are read, with their documents. The policies of the
//! groups a user is in are not read: the answer names the groups, so a page says what it did not look at.

use axum::extract::RawQuery;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;
use serde_json::{Value, json};

use super::resources::refused;
use super::{at, error, json, param};
use crate::model::{HeldPolicies, HeldPolicy};
use crate::resources::bus::Bus;
use crate::state::Shared;

/// The most policies of one kind read (one call each).
const MOST: usize = 20;

/// `text` with its `%XX` escapes decoded (IAM answers a policy document URL-encoded), and `+` left as it is.
fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        let hex = bytes
            .get(i + 1..i + 3)
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match (b, hex) {
            (b'%', Some(v)) => {
                out.push(v);
                i += 3;
            }
            _ => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn names<'a>(v: &'a Value, key: &str) -> impl Iterator<Item = &'a str> {
    at(v, key)
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
}

/// The managed policy `arn`'s default version, as JSON text.
async fn managed_document(bus: &Bus, arn: &str) -> Option<String> {
    let got = bus
        .call("iam", "GetPolicy", &json!({"PolicyArn": arn}))
        .await
        .ok()?;
    let version = at(at(&got, "Policy"), "DefaultVersionId").as_str()?;
    let got = bus
        .call(
            "iam",
            "GetPolicyVersion",
            &json!({"PolicyArn": arn, "VersionId": version}),
        )
        .await
        .ok()?;
    at(at(&got, "PolicyVersion"), "Document")
        .as_str()
        .map(decode)
}

async fn read(bus: &Bus, kind: &str, name: &str) -> Result<HeldPolicies, aws_wire::ApiError> {
    let (member, list, get, attached) = if kind == "user" {
        (
            "UserName",
            "ListUserPolicies",
            "GetUserPolicy",
            "ListAttachedUserPolicies",
        )
    } else {
        (
            "RoleName",
            "ListRolePolicies",
            "GetRolePolicy",
            "ListAttachedRolePolicies",
        )
    };
    let mut policies = Vec::new();
    let inline = bus.call("iam", list, &json!({member: name})).await?;
    for policy in names(&inline, "PolicyNames").take(MOST) {
        let got = bus
            .call("iam", get, &json!({member: name, "PolicyName": policy}))
            .await?;
        policies.push(HeldPolicy {
            name: policy.to_owned(),
            inline: true,
            arn: None,
            document: at(&got, "PolicyDocument").as_str().map(decode),
        });
    }
    let managed = bus.call("iam", attached, &json!({member: name})).await?;
    for p in at(&managed, "AttachedPolicies")
        .as_array()
        .into_iter()
        .flatten()
        .take(MOST)
    {
        let (Some(policy), Some(arn)) = (at(p, "PolicyName").as_str(), at(p, "PolicyArn").as_str())
        else {
            continue;
        };
        policies.push(HeldPolicy {
            name: policy.to_owned(),
            inline: false,
            arn: Some(arn.to_owned()),
            document: managed_document(bus, arn).await,
        });
    }
    let groups = if kind == "user" {
        let g = bus
            .call("iam", "ListGroupsForUser", &json!({"UserName": name}))
            .await?;
        at(&g, "Groups")
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|g| at(g, "GroupName").as_str().map(str::to_owned))
            .collect()
    } else {
        Vec::new()
    };
    Ok(HeldPolicies { policies, groups })
}

/// `GET /api/v1/access/policies`.
pub async fn policies(State(s): State<Shared>, RawQuery(query): RawQuery) -> Response {
    let (kind, name) = (
        param(query.as_deref(), "kind").unwrap_or_default(),
        decode(param(query.as_deref(), "name").unwrap_or_default()),
    );
    if !matches!(kind, "user" | "role") || name.is_empty() {
        return error(
            StatusCode::BAD_REQUEST,
            "BAD_REQUEST",
            "Policies are read for ?kind=user or ?kind=role and a ?name=.",
        );
    }
    let Some(bus) = s.bus() else {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "NO_BUS",
            "This console was started without the services' bus, so it cannot read IAM.",
        );
    };
    match read(&bus, kind, &name).await {
        Ok(held) => json(StatusCode::OK, &held),
        Err(e) => refused(&e),
    }
}

#[cfg(test)]
mod tests {
    use super::decode;

    #[test]
    fn a_url_encoded_document_is_read_back() {
        assert_eq!(
            decode("%7B%22Version%22%3A%222012-10-17%22%7D"),
            r#"{"Version":"2012-10-17"}"#
        );
        assert_eq!(decode("a%2"), "a%2");
        assert_eq!(decode("plain-name"), "plain-name");
    }
}
