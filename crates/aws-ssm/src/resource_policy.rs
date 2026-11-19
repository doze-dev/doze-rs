//! Resource policies: `PutResourcePolicy`, `GetResourcePolicies`, `DeleteResourcePolicy`, on a parameter or a document (the model
//! lists those two and OpsCenter's `OpsItemGroup`, which doze-aws does not run). They are what AWS shares a parameter with another
//! account by; here they are also what the IAM evaluator reads as the resource-based layer of every request on the parameter or
//! document (see `authz.rs`), as it reads an SQS queue's or a secret's policy.
//!
//! As the model says: a parameter takes a policy only in the advanced tier, and a `SecureString` only under a customer managed key; a
//! policy is at most 1024 bytes; changing or deleting one needs its `PolicyId` and the `PolicyHash` it has now. As AWS answered
//! (recorded 2026-11-19): a parameter holds one policy (a second put without an id is "already exists"), an id and hash that are not the
//! current pair are `ResourcePolicyNotFoundException` (not the model's conflict), and the policy is kept as compact JSON, its keys in the
//! order given. The id has a UUID's shape and the hash is 32 hex digits, as AWS's; both are derived from SHA-256 here (opaque on AWS).
//! Refusals not marked recorded are (memory).

use aws_model_ssm::keys::{
    DeleteResourcePolicyRequest as Delete, GetResourcePoliciesRequest as Get,
    GetResourcePoliciesResponse as Got, GetResourcePoliciesResponseEntry as Entry,
    PutResourcePolicyRequest as Put, PutResourcePolicyResponse as Done,
};
use aws_wire::ApiError;
use aws_wire::handler::{empty, reply, request};
use aws_wire::service::{Call, Reply};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::core::{Core, validation};
use crate::page;

/// The largest policy (the model's `ResourcePolicyLimitExceededException`: "A policy can't be greater than 1024 bytes in size").
const MAX_BYTES: usize = 1024;
/// The page `GetResourcePolicies` gives when the request names none (the model's most).
const PAGE: usize = 50;

/// One policy on a resource.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ResPolicy {
    pub(crate) id: String,
    pub(crate) hash: String,
    pub(crate) text: String,
}

/// The policies of one resource, in the order they were put.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct Held {
    pub(crate) policies: Vec<ResPolicy>,
}

/// What a resource ARN names.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Target<'a> {
    Parameter(&'a str),
    Document(&'a str),
}

impl Target<'_> {
    /// The key its policies are kept under.
    pub(crate) fn key(&self) -> String {
        match self {
            Self::Parameter(name) => format!("parameter:{}", name.trim_start_matches('/')),
            Self::Document(name) => format!("document:{name}"),
        }
    }
}

/// The key a parameter's policies are kept under.
pub(crate) fn parameter_key(name: &str) -> String {
    Target::Parameter(name).key()
}

/// The key a document's policies are kept under.
pub(crate) fn document_key(name: &str) -> String {
    Target::Document(name).key()
}

fn invalid(message: impl Into<String>, members: &[&str]) -> ApiError {
    let names = serde_json::to_string(members).unwrap_or_else(|_| "[]".to_owned());
    ApiError::modeled("ResourcePolicyInvalidParameterException", message)
        .with_body("ParameterNames", names)
}

fn not_found(arn: &str) -> ApiError {
    // (memory): a document, or an ARN outside the caller's account and region.
    ApiError::modeled(
        "ResourceNotFoundException",
        format!("The resource {arn} was not found."),
    )
}

fn parameter_not_found(arn: &str) -> ApiError {
    // Recorded 2026-11-19.
    ApiError::modeled(
        "ResourceNotFoundException",
        format!("Parameter {arn} not found"),
    )
}

/// No policy on the resource has this id and this hash: a stale hash is answered so too (recorded 2026-11-19).
fn no_policy(arn: &str, id: &str, hash: &str) -> ApiError {
    // Recorded 2026-11-19.
    ApiError::modeled(
        "ResourcePolicyNotFoundException",
        format!(
            "Resource policy for resource {arn} with policy ID {id} and policy hash {hash} does not exist"
        ),
    )
}

/// A parameter holds one policy; a second needs the first's id and hash, to change it (recorded 2026-11-19).
fn already_held(arn: &str) -> ApiError {
    // Recorded 2026-11-19.
    validation(format!(
        "Resource policy for parameter {arn} already exists"
    ))
}

/// What `arn` names, in the caller's own account and region.
fn target<'a>(arn: &'a str, call: &Call) -> Result<Target<'a>, ApiError> {
    let prefix = format!(
        "arn:aws:ssm:{}:{}:",
        call.caller.region, call.caller.account_id
    );
    let Some(rest) = arn.strip_prefix(prefix.as_str()) else {
        return Err(not_found(arn));
    };
    if let Some(name) = rest.strip_prefix("parameter/") {
        return Ok(Target::Parameter(name));
    }
    if let Some(name) = rest.strip_prefix("document/") {
        return Ok(Target::Document(name));
    }
    if rest.starts_with("opsitemgroup/") {
        return Err(invalid(
            "doze-aws does not run OpsCenter, so an OpsItemGroup takes no policy. Parameters and documents do.",
            &["ResourceArn"],
        ));
    }
    Err(invalid(
        format!(
            "{arn} is not the ARN of a resource that takes a resource policy (a parameter or a document)."
        ),
        &["ResourceArn"],
    ))
}

/// 32 hex digits, the shape of AWS's `PolicyHash`.
fn hash_of(text: &str) -> String {
    let digest = Sha256::digest(text.as_bytes());
    aws_core::hex::encode(digest.get(..16).unwrap_or_default())
}

/// A UUID's shape (version 4, RFC 4122 variant), the shape of AWS's `PolicyId`, derived from `seed`.
fn id_of(seed: &str) -> String {
    let mut b = [0u8; 16];
    b.copy_from_slice(
        Sha256::digest(seed.as_bytes())
            .get(..16)
            .unwrap_or(&[0; 16]),
    );
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h = aws_core::hex::encode(&b);
    format!(
        "{}-{}-{}-{}-{}",
        &h[..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..]
    )
}

/// The policy as AWS keeps it (recorded 2026-11-19): compact JSON, every key and value in the order given. Whitespace outside a string
/// is dropped; the text is already known to parse.
fn compact(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let (mut quoted, mut escaped) = (false, false);
    for c in text.chars() {
        if quoted {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                quoted = false;
            }
        } else if c == '"' {
            quoted = true;
            out.push(c);
        } else if !c.is_whitespace() {
            out.push(c);
        }
    }
    out
}

/// Refuses what is not an IAM policy document, or is larger than AWS takes.
fn check(text: &str) -> Result<(), ApiError> {
    if text.len() > MAX_BYTES {
        // (memory)
        return Err(ApiError::modeled(
            "ResourcePolicyLimitExceededException",
            format!(
                "The policy is {} bytes; a resource policy can't be greater than {MAX_BYTES} bytes.",
                text.len()
            ),
        )
        .with_body("Limit", MAX_BYTES.to_string())
        .with_body("LimitType", "\"PolicySize\""));
    }
    match aws_iam_policy::parse(text) {
        Some(Ok(_)) => Ok(()),
        // (memory)
        Some(Err(why)) => Err(ApiError::modeled(
            "MalformedResourcePolicyDocumentException",
            format!("The policy document is not valid: {why}."),
        )),
        None => Err(ApiError::modeled(
            "MalformedResourcePolicyDocumentException",
            "The policy document is not valid JSON.",
        )),
    }
}

impl Core {
    /// The parameter a policy's ARN names, as it is kept (`/a/b`, or a name without a slash).
    fn parameter_named(&self, tail: &str) -> Option<String> {
        let slashed = format!("/{tail}");
        if self.params.get(&slashed).is_some() {
            return Some(slashed);
        }
        (!tail.contains('/') && self.params.get(tail).is_some()).then(|| tail.to_owned())
    }

    /// Refuses a resource that does not exist or cannot take a policy.
    fn may_hold(&self, t: &Target<'_>, arn: &str) -> Result<(), ApiError> {
        match t {
            Target::Document(name) => self
                .documents
                .get(name)
                .map(|_| ())
                .ok_or_else(|| not_found(arn)),
            Target::Parameter(tail) => {
                let name = self
                    .parameter_named(tail)
                    .ok_or_else(|| parameter_not_found(arn))?;
                let latest = self.params.get(&name).map(|p| &p.latest);
                if latest.is_some_and(|v| !v.advanced) {
                    // Recorded 2026-11-19; asked before the policy is read, so a policy that is not JSON is refused so too.
                    return Err(validation("Only advanced parameters can be shared"));
                }
                if latest.is_some_and(|v| v.key_id.as_deref() == Some(crate::kms::DEFAULT_KEY)) {
                    return Err(invalid(
                        "A SecureString parameter encrypted with the AWS managed key alias/aws/ssm can't be shared. Encrypt it with a customer managed key and try again.",
                        &["ResourceArn"],
                    ));
                }
                Ok(())
            }
        }
    }

    pub(crate) fn put_resource_policy(&mut self, call: &Call) -> Reply {
        let v = request::<Put>(call)?;
        let arn = v.get(Put::RESOURCE_ARN).unwrap_or_default();
        let text = v.get(Put::POLICY).unwrap_or_default();
        let t = target(arn, call)?;
        self.may_hold(&t, arn)?;
        check(text)?;
        let text = compact(text);
        let key = t.key();
        let hash = hash_of(&text);
        let mut held = self.res_policies.get(&key).cloned().unwrap_or_default();
        let id = match (v.get(Put::POLICY_ID), v.get(Put::POLICY_HASH)) {
            (None, None) => {
                if matches!(t, Target::Parameter(_)) && !held.policies.is_empty() {
                    return Err(already_held(arn));
                }
                let id = id_of(&format!(
                    "{key}\n{}\n{}",
                    self.now_ms(),
                    held.policies.len()
                ));
                held.policies.push(ResPolicy {
                    id: id.clone(),
                    hash: hash.clone(),
                    text: text.clone(),
                });
                id
            }
            (Some(_), None) => {
                return Err(invalid(
                    "PolicyHash is required to update a policy.",
                    &["PolicyHash"],
                ));
            }
            (None, Some(_)) => {
                return Err(invalid(
                    "PolicyId is required to update a policy.",
                    &["PolicyId"],
                ));
            }
            (Some(id), Some(given)) => {
                let p = held
                    .policies
                    .iter_mut()
                    .find(|p| p.id == id && p.hash == given)
                    .ok_or_else(|| no_policy(arn, id, given))?;
                p.hash.clone_from(&hash);
                p.text = text;
                id.to_owned()
            }
        };
        self.res_policies.insert(key, held);
        reply::<Done>(|o| {
            o.put(Done::POLICY_ID, &id);
            o.put(Done::POLICY_HASH, &hash);
        })
    }

    pub(crate) fn get_resource_policies(&mut self, call: &Call) -> Reply {
        let v = request::<Get>(call)?;
        let arn = v.get(Get::RESOURCE_ARN).unwrap_or_default();
        let t = target(arn, call)?;
        match &t {
            Target::Document(name) if self.documents.get(name).is_none() => {
                return Err(not_found(arn));
            }
            Target::Parameter(tail) if self.parameter_named(tail).is_none() => {
                return Err(parameter_not_found(arn));
            }
            _ => {}
        }
        let limit = v
            .get(Get::MAX_RESULTS)
            .and_then(|n| usize::try_from(n).ok())
            .unwrap_or(PAGE);
        let after = v.get(Get::NEXT_TOKEN).map(page::resume).transpose()?;
        let held = self.res_policies.get(&t.key());
        let all = held.map_or(&[][..], |h| h.policies.as_slice());
        let skip = after.map_or(0, |id| {
            all.iter()
                .position(|p| p.id == id)
                .map_or(all.len(), |i| i + 1)
        });
        let rest = all.get(skip..).unwrap_or_default();
        let shown = rest.get(..limit.min(rest.len())).unwrap_or_default();
        let token = (rest.len() > shown.len())
            .then(|| shown.last().map(|p| page::token(&p.id)))
            .flatten();
        reply::<Got>(|o| {
            o.put_list(Got::POLICIES, |l| {
                for p in shown {
                    l.push(|e| {
                        e.put(Entry::POLICY_ID, &p.id);
                        e.put(Entry::POLICY_HASH, &p.hash);
                        e.put(Entry::POLICY, &p.text);
                    });
                }
            });
            if let Some(t) = &token {
                o.put(Got::NEXT_TOKEN, t);
            }
        })
    }

    pub(crate) fn delete_resource_policy(&mut self, call: &Call) -> Reply {
        let v = request::<Delete>(call)?;
        let arn = v.get(Delete::RESOURCE_ARN).unwrap_or_default();
        let t = target(arn, call)?;
        let key = t.key();
        let (id, given) = (
            v.get(Delete::POLICY_ID).unwrap_or_default(),
            v.get(Delete::POLICY_HASH).unwrap_or_default(),
        );
        let mut held = self.res_policies.get(&key).cloned().unwrap_or_default();
        let ix = held
            .policies
            .iter()
            .position(|p| p.id == id && p.hash == given)
            .ok_or_else(|| no_policy(arn, id, given))?;
        held.policies.remove(ix);
        if held.policies.is_empty() {
            self.res_policies.remove(&key);
        } else {
            self.res_policies.insert(key, held);
        }
        empty()
    }

    /// The policy on the resource kept under `key`, as one document for the evaluator: the statements of every policy it holds.
    pub(crate) fn policy_of(&self, key: &str) -> Option<String> {
        let held = self.res_policies.get(key)?;
        if let [one] = held.policies.as_slice() {
            return Some(one.text.clone());
        }
        let statements: Vec<Value> = held
            .policies
            .iter()
            .filter_map(|p| serde_json::from_str::<Value>(&p.text).ok())
            .flat_map(|doc| match doc.get("Statement") {
                Some(Value::Array(list)) => list.clone(),
                Some(one) => vec![one.clone()],
                None => Vec::new(),
            })
            .collect();
        Some(serde_json::json!({"Version": "2012-10-17", "Statement": statements}).to_string())
    }

    /// Forgets the policies of a resource that is gone, so a new one of the same name starts with none.
    pub(crate) fn forget_policies(&mut self, key: &str) {
        self.res_policies.remove(key);
    }
}
