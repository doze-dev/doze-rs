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
    }
    let public = crate::public::is_public(&doc);
    core.require(req.bucket())?;
    let blocked = core.blocks(req.bucket(), "BlockPublicPolicy");
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

/// Whether a policy document makes its bucket public (`GetBucketPolicyStatus`): AWS's rule, in [`crate::public`].
pub(crate) fn grants_public(doc: &Value) -> bool {
    crate::public::is_public(doc)
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
