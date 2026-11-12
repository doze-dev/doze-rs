//! The checks AWS makes of a configuration before it keeps it, and what a bucket answers for a configuration nobody set.

use aws_wire::ApiError;
use serde_json::{Value, json};

use crate::core::Core;
use crate::errors::{malformed_policy, malformed_xml};
use crate::req::Req;

/// What a bucket answers for a configuration nobody set, where AWS answers with something and not with an error. Wording and values
/// come from the recordings for encryption and the public access block; the rest are the documented defaults (memory).
pub(crate) fn defaults(kind: &str) -> Option<Value> {
    Some(match kind {
        "encryption" => json!({"Rules": [{
            "ApplyServerSideEncryptionByDefault": {"SSEAlgorithm": "AES256"},
            "BucketKeyEnabled": false,
            "BlockedEncryptionTypes": {"EncryptionType": ["SSE-C"]},
        }]}),
        "public-access-block" => json!({
            "BlockPublicAcls": true, "IgnorePublicAcls": true, "BlockPublicPolicy": true, "RestrictPublicBuckets": true,
        }),
        "ownership" => json!({"Rules": [{"ObjectOwnership": "BucketOwnerEnforced"}]}),
        "request-payment" => json!({"Payer": "BucketOwner"}),
        "logging" | "accelerate" | "notification" => json!({}),
        _ => return None,
    })
}

/// Checks `body`, the configuration a request carries for `kind`, and may fill in what AWS fills in (notification ids).
pub(crate) fn check(
    core: &Core,
    req: &Req<'_>,
    kind: &str,
    body: &mut Value,
) -> Result<(), ApiError> {
    match kind {
        "policy" => policy(core, req, body),
        "cors" => cors(body),
        "object-lock" => crate::lock::check_configuration(core, req, body),
        "lifecycle" => crate::lifecycle_check::rules(body),
        "encryption" => one_of(
            body.pointer("/Rules/0/ApplyServerSideEncryptionByDefault/SSEAlgorithm"),
            &["AES256", "aws:kms", "aws:kms:dsse"],
        ),
        "ownership" => one_of(
            body.pointer("/Rules/0/ObjectOwnership"),
            &[
                "BucketOwnerEnforced",
                "BucketOwnerPreferred",
                "ObjectWriter",
            ],
        ),
        "accelerate" => one_of(body.get("Status"), &["Enabled", "Suspended"]),
        "request-payment" => one_of(body.get("Payer"), &["BucketOwner", "Requester"]),
        "website" => {
            if body.get("IndexDocument").is_none() && body.get("RedirectAllRequestsTo").is_none() {
                return Err(malformed_xml());
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn one_of(value: Option<&Value>, allowed: &[&str]) -> Result<(), ApiError> {
    match value.and_then(Value::as_str) {
        Some(v) if allowed.contains(&v) => Ok(()),
        _ => Err(malformed_xml()),
    }
}

fn cors(body: &Value) -> Result<(), ApiError> {
    let rules = body
        .get("CORSRules")
        .and_then(Value::as_array)
        .filter(|r| !r.is_empty() && r.len() <= 100)
        .ok_or_else(malformed_xml)?;
    for r in rules {
        let methods = r
            .get("AllowedMethods")
            .and_then(Value::as_array)
            .ok_or_else(malformed_xml)?;
        for m in methods.iter().filter_map(Value::as_str) {
            if !["GET", "PUT", "POST", "DELETE", "HEAD"].contains(&m) {
                return Err(ApiError::new(
                    400,
                    "InvalidRequest",
                    format!(
                        "Found unsupported HTTP method in CORS config. Unsupported method is {m}"
                    ),
                ));
            }
        }
        if r.get("AllowedOrigins")
            .and_then(Value::as_array)
            .is_none_or(Vec::is_empty)
        {
            return Err(malformed_xml());
        }
    }
    Ok(())
}

/// A bucket policy: JSON, with statements that name a principal and only this bucket's resources; a public one is refused while
/// the bucket blocks public policies.
fn policy(core: &Core, req: &Req<'_>, body: &Value) -> Result<(), ApiError> {
    let text = body.as_str().ok_or_else(malformed_xml)?;
    let doc: Value = serde_json::from_str(text)
        .map_err(|_| malformed_policy("This policy contains invalid Json"))?;
    let statements = match doc.get("Statement") {
        Some(Value::Array(a)) => a.clone(),
        Some(o @ Value::Object(_)) => vec![o.clone()],
        _ => return Err(malformed_policy("Syntax error in policy.")),
    };
    let arn = format!("arn:aws:s3:::{}", req.bucket());
    let mut public = false;
    for s in &statements {
        if s.get("Principal").is_none() && s.get("NotPrincipal").is_none() {
            return Err(malformed_policy("Missing required field Principal"));
        }
        let resources = strings(s.get("Resource"));
        if resources.is_empty() && s.get("NotResource").is_none() {
            return Err(malformed_policy("Missing required field Resource"));
        }
        if resources
            .iter()
            .any(|r| *r != "*" && *r != arn && !r.starts_with(&format!("{arn}/")))
        {
            return Err(malformed_policy("Policy has invalid resource"));
        }
        public |= statement_is_public(s);
    }
    let blocked = core
        .require(req.bucket())?
        .configs
        .get("public-access-block")
        .and_then(|c| serde_json::from_str::<Value>(c).ok())
        .is_none_or(|c| c.get("BlockPublicPolicy").and_then(Value::as_bool) != Some(false));
    if public && blocked {
        // (memory) AWS's text for a public policy refused by the block.
        return Err(ApiError::new(
            403,
            "AccessDenied",
            format!(
                "User: arn:aws:iam::{}:root is not authorized to perform: s3:PutBucketPolicy on resource: \"{arn}\" because public policies are blocked by the BlockPublicPolicy block public access setting.",
                req.account()
            ),
        ));
    }
    Ok(())
}

/// Whether a statement grants to everyone: an `Allow` for principal `*` (or `{"AWS": "*"}`) with no `Condition`. AWS also counts a
/// condition that does not name the caller as leaving it public, and weighs `Deny` statements; this is the narrower rule the Go
/// edition used and the stage's `BlockPublicPolicy` check already applies. (memory)
fn statement_is_public(s: &Value) -> bool {
    s.get("Effect").and_then(Value::as_str) == Some("Allow")
        && s.get("Condition").is_none()
        && (strings(s.get("Principal")).contains(&"*".to_owned())
            || strings(s.pointer("/Principal/AWS")).contains(&"*".to_owned()))
}

/// Whether a policy document makes its bucket public (`GetBucketPolicyStatus`).
pub(crate) fn grants_public(doc: &Value) -> bool {
    match doc.get("Statement") {
        Some(Value::Array(a)) => a.iter().any(statement_is_public),
        Some(s @ Value::Object(_)) => statement_is_public(s),
        _ => false,
    }
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
