//! Resource-based policies: `PutResourcePolicy`, `GetResourcePolicy`, `DeleteResourcePolicy`, and `ResourcePolicy` on `CreateTable`,
//! on a table or its stream. A policy is stored, and handed to IAM with every request that acts on its table (an index is governed by
//! its table's policy, as the model says) or its stream, so IAM weighs it with the caller's identity policies as it does a queue's or a
//! bucket's: an explicit deny in it refuses, and an allow in it is enough for a caller of the same account.
//!
//! AWS applies a policy eventually (a get right after a put may not see it); here it is in place when the call answers. The revision
//! id is the time of the change in milliseconds, a string as the model says to compare it. AWS hands a policy back without the
//! whitespace between its tokens (recorded 2026-11-19), so it is stored that way. A message marked `Recorded 2026-11-19` is AWS's;
//! the rest are `(memory)`.

use aws_peers::Need;
use aws_wire::ApiError;
use aws_wire::service::Call;
use serde_json::{Value as Json, json};

use crate::core::{Core, Cx};
use crate::extras::{Extra, Policy, policy_key};
use crate::req::{bad_param, str_of, table_arn, table_not_found, validation};

/// The most a policy document may hold, whitespace counted (the model: 20 KB).
const MAX_POLICY_BYTES: usize = 20 * 1024;
/// The revision a conditional put gives to say no policy may exist yet.
const NO_POLICY: &str = "NO_POLICY";

fn policy_not_found(arn: &str) -> ApiError {
    ApiError::modeled(
        "PolicyNotFoundException",
        format!("Resource-based policy not found for the provided ResourceArn: {arn}"),
    )
}

fn revision_mismatch(arn: &str, table: &str, expected: &str, actual: Option<&str>) -> ApiError {
    let Some(actual) = actual else {
        // `(memory)`: a revision asked of a resource with no policy is a policy not found.
        return policy_not_found(arn);
    };
    // Recorded 2026-11-19 (both).
    let detail = if expected == NO_POLICY {
        format!(
            "Requested policy update expecting none to be present for table {table}, but a policy exists with revision id {actual}."
        )
    } else {
        format!(
            "Requested update for policy with revision id {expected}, but the policy associated to target table {table} has revision id {actual}."
        )
    };
    ApiError::modeled(
        "PolicyNotFoundException",
        format!("Resource-based policy not found for the provided ResourceArn: {detail}"),
    )
}

/// A JSON text without the whitespace between its tokens; strings are kept as written.
fn compact(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let (mut in_string, mut escaped) = (false, false);
    for c in text.chars() {
        if in_string {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
        } else if c == '"' {
            in_string = true;
            out.push(c);
        } else if !c.is_whitespace() {
            out.push(c);
        }
    }
    out
}

/// Checks a policy document as IAM checks one on a resource, and its size.
pub(crate) fn check_policy(text: &str) -> Result<(), ApiError> {
    if text.len() > MAX_POLICY_BYTES {
        return Err(validation(format!(
            "Resource-based policy document exceeds the maximum allowed size of {MAX_POLICY_BYTES} bytes"
        )));
    }
    // Recorded 2026-11-19 for a document that is not JSON; the other refusals are taken to carry the same prefix.
    if serde_json::from_str::<Json>(text).is_err() {
        return Err(bad_param(
            "Invalid policy document: This policy contains invalid Json",
        ));
    }
    aws_iam_policy::check(text, aws_iam_policy::Kind::Resource)
        .map(drop)
        .map_err(|e| bad_param(&format!("Invalid policy document: {}", e.0)))
}

impl Core {
    /// The table a policy's ARN names (the table's own, or its current stream's), checked to exist; and the canonical ARN.
    fn policy_target(&self, cx: &Cx<'_>) -> Result<(String, String), ApiError> {
        let arn = str_of(&cx.body, "ResourceArn").unwrap_or_default();
        let missing = || {
            ApiError::modeled(
                "ResourceNotFoundException",
                format!("Requested resource not found: {arn}"),
            )
        };
        if !arn.starts_with("arn:") {
            return Err(validation(format!(
                "Invalid ResourceArn provided as input {arn}"
            )));
        }
        let name = crate::req::table_name(arn, cx.region(), cx.account()).map_err(|_| missing())?;
        // Recorded 2026-11-19: a table that is not there is named.
        let t = self.tables.get(name).ok_or_else(|| table_not_found(name))?;
        if let Some((_, label)) = arn.split_once("/stream/") {
            let current = t.stream.as_ref().is_some_and(|s| s.label == label);
            if !current {
                return Err(missing());
            }
        } else if arn != table_arn(cx.region(), cx.account(), name) {
            return Err(validation(format!(
                "Invalid ResourceArn provided as input {arn}"
            )));
        }
        Ok((name.to_owned(), arn.to_owned()))
    }

    fn policy(&self, arn: &str) -> Option<&Policy> {
        match self.extras.get(&policy_key(arn)) {
            Some(Extra::Policy(p)) => Some(p),
            _ => None,
        }
    }

    pub(crate) fn put_resource_policy(&mut self, cx: &Cx<'_>) -> Result<Json, ApiError> {
        let (table, arn) = self.policy_target(cx)?;
        let text = str_of(&cx.body, "Policy").unwrap_or_default();
        check_policy(text)?;
        let current = self.policy(&arn).cloned();
        if let Some(expected) = str_of(&cx.body, "ExpectedRevisionId") {
            let actual = current.as_ref().map(|p| p.revision.as_str());
            let matches = if expected == NO_POLICY {
                actual.is_none()
            } else {
                actual == Some(expected)
            };
            if !matches {
                return Err(revision_mismatch(&arn, &table, expected, actual));
            }
        }
        let revision = self.store_policy(&arn, text, cx.now_ms);
        Ok(json!({ "RevisionId": revision }))
    }

    /// Stores a policy; the same document again keeps its revision.
    pub(crate) fn store_policy(&mut self, arn: &str, text: &str, now_ms: i64) -> String {
        let text = compact(text);
        if let Some(p) = self.policy(arn).filter(|p| p.text == text) {
            return p.revision.clone();
        }
        let last = self
            .policy(arn)
            .and_then(|p| p.revision.parse::<i64>().ok())
            .unwrap_or(0);
        let revision = now_ms.max(last + 1).to_string();
        self.extras.insert(
            policy_key(arn),
            Extra::Policy(Policy {
                text,
                revision: revision.clone(),
            }),
        );
        revision
    }

    pub(crate) fn get_resource_policy(&mut self, cx: &Cx<'_>) -> Result<Json, ApiError> {
        let (_, arn) = self.policy_target(cx)?;
        let p = self.policy(&arn).ok_or_else(|| policy_not_found(&arn))?;
        Ok(json!({"Policy": p.text, "RevisionId": p.revision}))
    }

    pub(crate) fn delete_resource_policy(&mut self, cx: &Cx<'_>) -> Result<Json, ApiError> {
        let (table, arn) = self.policy_target(cx)?;
        let current = self.policy(&arn).map(|p| p.revision.clone());
        if let Some(expected) = str_of(&cx.body, "ExpectedRevisionId")
            && current.as_deref() != Some(expected)
        {
            return Err(revision_mismatch(
                &arn,
                &table,
                expected,
                current.as_deref(),
            ));
        }
        self.extras.remove(&policy_key(&arn));
        // With no policy there, the call does nothing and says so with no revision (the model).
        Ok(current.map_or_else(|| json!({}), |r| json!({ "RevisionId": r })))
    }

    /// Forgets the policies of a table and of its streams.
    pub(crate) fn drop_policies(&mut self, table: &str) {
        let marker = format!(":table/{table}");
        let gone: Vec<String> = self
            .extras
            .with_prefix("policy/")
            .map(|(k, _)| k.to_owned())
            .filter(|k| {
                k.split_once(&marker)
                    .is_some_and(|(_, rest)| rest.is_empty() || rest.starts_with("/stream/"))
            })
            .collect();
        for k in gone {
            self.extras.remove(&k);
        }
    }

    /// What a call asks to do, each with the policy of the table or stream it acts on.
    pub(crate) fn needs(&self, call: &Call) -> Vec<Need> {
        crate::authz::needs(call)
            .into_iter()
            .map(|need| {
                let policy = self.policy_for(&need.resource).map(|p| p.text.clone());
                need.with_policy(policy)
            })
            .collect()
    }

    /// The text of the policy that governs `resource`.
    pub(crate) fn policy_text(&self, resource: &str) -> Option<String> {
        self.policy_for(resource).map(|p| p.text.clone())
    }

    /// The policy that governs `resource`: a stream's own, or the table's for the table and its indexes.
    fn policy_for(&self, resource: &str) -> Option<&Policy> {
        if resource.contains("/stream/") {
            return self.policy(resource);
        }
        let table = match resource.split_once("/index/") {
            Some((t, _)) => t,
            None => resource,
        };
        self.policy(table)
    }
}
