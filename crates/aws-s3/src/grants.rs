//! The grants of an access control list: canned ACLs, `x-amz-grant-*` headers and `AccessControlPolicy` bodies read into them, the
//! answer `Get*Acl` gives, and what an `AllUsers` grant allows, as policy statements the authorizer can weigh.

use aws_wire::ApiError;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::buckets::owner;
use crate::errors::{invalid_argument, malformed_xml};
use crate::req::Req;

pub(crate) const ALL_USERS: &str = "http://acs.amazonaws.com/groups/global/AllUsers";
const AUTHENTICATED_USERS: &str = "http://acs.amazonaws.com/groups/global/AuthenticatedUsers";
const LOG_DELIVERY: &str = "http://acs.amazonaws.com/groups/s3/LogDelivery";
const PERMISSIONS: &[&str] = &["FULL_CONTROL", "WRITE", "WRITE_ACP", "READ", "READ_ACP"];

/// Who a grant is to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Grantee {
    /// A canonical user id.
    User(String),
    /// A predefined group, by its URI.
    Group(String),
}

/// One grant of an access control list.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Grant {
    pub(crate) grantee: Grantee,
    pub(crate) permission: String,
}

/// A bucket's Object Ownership.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Ownership {
    Enforced,
    Preferred,
    Writer,
}

impl Ownership {
    pub(crate) fn parse(s: &str) -> Option<Self> {
        match s {
            "BucketOwnerEnforced" => Some(Self::Enforced),
            "BucketOwnerPreferred" => Some(Self::Preferred),
            "ObjectWriter" => Some(Self::Writer),
            _ => None,
        }
    }
}

/// The canonical id of the local account (the owner of every bucket and object).
pub(crate) fn owner_id(account: &str) -> String {
    owner(account)
        .get("ID")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn grant(grantee: Grantee, permission: &str) -> Grant {
    Grant {
        grantee,
        permission: permission.to_owned(),
    }
}

/// The grants a canned ACL stands for. The bucket owner and the object owner are the one local account, so the `bucket-owner-*`
/// forms come to the owner's full control (memory: AWS lists the owner once in that case).
fn canned(name: &str, owner: &str) -> Option<Vec<Grant>> {
    let mut grants = vec![grant(Grantee::User(owner.to_owned()), "FULL_CONTROL")];
    let group = |uri: &str| Grantee::Group(uri.to_owned());
    match name {
        "private" | "bucket-owner-read" | "bucket-owner-full-control" | "aws-exec-read" => {}
        "public-read" => grants.push(grant(group(ALL_USERS), "READ")),
        "public-read-write" => {
            grants.push(grant(group(ALL_USERS), "READ"));
            grants.push(grant(group(ALL_USERS), "WRITE"));
        }
        "authenticated-read" => grants.push(grant(group(AUTHENTICATED_USERS), "READ")),
        "log-delivery-write" => {
            grants.push(grant(group(LOG_DELIVERY), "WRITE"));
            grants.push(grant(group(LOG_DELIVERY), "READ_ACP"));
        }
        _ => return None,
    }
    Some(grants)
}

/// `id="…", uri="…"` (an `x-amz-grant-*` header) as grantees. An e-mail grantee cannot be resolved to an account here.
fn header_grantees(value: &str) -> Result<Vec<Grantee>, ApiError> {
    value
        .split(',')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (kind, v) = p
                .split_once('=')
                .ok_or_else(|| invalid_argument("Invalid grant"))?;
            let v = v.trim().trim_matches('"').to_owned();
            match kind.trim() {
                "id" => Ok(Grantee::User(v)),
                "uri" => Ok(Grantee::Group(v)),
                "emailAddress" => Err(unresolvable_email()),
                _ => Err(invalid_argument("Invalid grant")),
            }
        })
        .collect()
}

fn unresolvable_email() -> ApiError {
    // (memory) AWS's refusal of an e-mail grantee it cannot find (and e-mail grantees are being retired).
    ApiError::new(
        400,
        "UnresolvableGrantByEmailAddress",
        "The e-mail address you provided does not match any account on record.",
    )
}

/// The grants a request's `x-amz-grant-*` headers name, in permission order.
fn header_grants(req: &Req<'_>) -> Result<Vec<Grant>, ApiError> {
    let mut out = Vec::new();
    for (member, permission) in [
        ("GrantFullControl", "FULL_CONTROL"),
        ("GrantWrite", "WRITE"),
        ("GrantWriteACP", "WRITE_ACP"),
        ("GrantRead", "READ"),
        ("GrantReadACP", "READ_ACP"),
    ] {
        if let Some(v) = req.s(member) {
            out.extend(
                header_grantees(v)?
                    .into_iter()
                    .map(|g| grant(g, permission)),
            );
        }
    }
    Ok(out)
}

/// The grants of an `AccessControlPolicy` body.
fn body_grants(policy: &Value) -> Result<Vec<Grant>, ApiError> {
    let list = policy
        .get("Grants")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    list.iter()
        .map(|g| {
            let permission = g
                .get("Permission")
                .and_then(Value::as_str)
                .filter(|p| PERMISSIONS.contains(p))
                .ok_or_else(malformed_xml)?;
            let who = g.get("Grantee").ok_or_else(malformed_xml)?;
            let text = |k: &str| who.get(k).and_then(Value::as_str).map(str::to_owned);
            let grantee = match text("Type").as_deref() {
                Some("CanonicalUser") => Grantee::User(text("ID").ok_or_else(malformed_xml)?),
                Some("Group") => Grantee::Group(text("URI").ok_or_else(malformed_xml)?),
                Some("AmazonCustomerByEmail") => return Err(unresolvable_email()),
                _ => return Err(malformed_xml()),
            };
            Ok(grant(grantee, permission))
        })
        .collect()
}

/// The ACL a request asks for: a canned one (`x-amz-acl`), grant headers, or (for `Put*Acl`) a body. `None` when it names none.
pub(crate) fn requested(req: &Req<'_>, owner: &str) -> Result<Option<Vec<Grant>>, ApiError> {
    let headers = header_grants(req)?;
    let body = req.get("AccessControlPolicy");
    let canned_name = req.s("ACL");
    if canned_name.is_some() && !headers.is_empty() {
        // (memory)
        return Err(ApiError::new(
            400,
            "InvalidRequest",
            "Specifying both Canned ACLs and Header Grants is not allowed",
        ));
    }
    if body.is_some() && (canned_name.is_some() || !headers.is_empty()) {
        // (memory)
        return Err(ApiError::new(
            400,
            "UnexpectedContent",
            "This request does not support content",
        ));
    }
    if let Some(name) = canned_name {
        return canned(name, owner)
            .map(Some)
            .ok_or_else(|| invalid_argument("Invalid canned ACL"));
    }
    if let Some(policy) = body {
        return body_grants(policy).map(Some);
    }
    Ok((!headers.is_empty()).then_some(headers))
}

/// Whether an ACL grants nothing but the owner's full control (what a bucket with ACLs disabled accepts).
pub(crate) fn owner_only(grants: &[Grant], owner: &str) -> bool {
    grants
        .iter()
        .all(|g| g.grantee == Grantee::User(owner.to_owned()) && g.permission == "FULL_CONTROL")
}

/// Whether an ACL grants anything to everyone or to every AWS account.
pub(crate) fn is_public(grants: &[Grant]) -> bool {
    grants.iter().any(
        |g| matches!(&g.grantee, Grantee::Group(u) if u == ALL_USERS || u == AUTHENTICATED_USERS),
    )
}

/// `GetBucketAcl`'s and `GetObjectAcl`'s answer: the owner and the grants.
pub(crate) fn describe(grants: Option<&[Grant]>, account: &str) -> Value {
    let me = owner(account);
    let id = owner_id(account);
    let default = [grant(Grantee::User(id), "FULL_CONTROL")];
    let grants: Vec<Value> = grants
        .unwrap_or(&default)
        .iter()
        .map(|g| {
            let grantee = match &g.grantee {
                Grantee::User(u) if Some(u.as_str()) == me.get("ID").and_then(Value::as_str) => {
                    let mut who = me.clone();
                    if let Some(o) = who.as_object_mut() {
                        o.insert("Type".into(), json!("CanonicalUser"));
                    }
                    who
                }
                Grantee::User(u) => json!({"ID": u, "Type": "CanonicalUser"}),
                Grantee::Group(uri) => json!({"URI": uri, "Type": "Group"}),
            };
            json!({"Grantee": grantee, "Permission": g.permission})
        })
        .collect();
    json!({"Owner": me, "Grants": grants})
}

/// The S3 actions an `AllUsers` grant of `permission` allows, on a bucket or on an object.
fn actions(permission: &str, on_bucket: bool) -> &'static [&'static str] {
    match (permission, on_bucket) {
        ("READ", true) => &[
            "s3:ListBucket",
            "s3:ListBucketVersions",
            "s3:ListBucketMultipartUploads",
        ],
        ("WRITE", true) => &["s3:PutObject", "s3:DeleteObject", "s3:DeleteObjectVersion"],
        ("READ_ACP", true) => &["s3:GetBucketAcl"],
        ("WRITE_ACP", true) => &["s3:PutBucketAcl"],
        ("READ", false) => &["s3:GetObject", "s3:GetObjectVersion"],
        ("READ_ACP", false) => &["s3:GetObjectAcl", "s3:GetObjectVersionAcl"],
        ("WRITE_ACP", false) => &["s3:PutObjectAcl", "s3:PutObjectVersionAcl"],
        ("FULL_CONTROL", true) => &[
            "s3:ListBucket",
            "s3:ListBucketVersions",
            "s3:ListBucketMultipartUploads",
            "s3:PutObject",
            "s3:DeleteObject",
            "s3:DeleteObjectVersion",
            "s3:GetBucketAcl",
            "s3:PutBucketAcl",
        ],
        ("FULL_CONTROL", false) => &[
            "s3:GetObject",
            "s3:GetObjectVersion",
            "s3:GetObjectAcl",
            "s3:GetObjectVersionAcl",
            "s3:PutObjectAcl",
            "s3:PutObjectVersionAcl",
        ],
        _ => &[],
    }
}

/// The policy statements that say what an ACL's `AllUsers` grants allow, so the authorizer weighs a public ACL as it weighs a public
/// bucket policy. A bucket's `WRITE` covers every key in it.
pub(crate) fn statements(grants: &[Grant], resource: &str, on_bucket: bool) -> Vec<Value> {
    grants
        .iter()
        .filter(|g| matches!(&g.grantee, Grantee::Group(u) if u == ALL_USERS))
        .map(|g| {
            let resource = if on_bucket && g.permission != "READ" && g.permission != "READ_ACP" {
                vec![resource.to_owned(), format!("{resource}/*")]
            } else {
                vec![resource.to_owned()]
            };
            json!({"Effect": "Allow", "Principal": "*", "Action": actions(&g.permission, on_bucket), "Resource": resource})
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canned_acls_expand_to_their_grants() {
        let public = canned("public-read", "me").unwrap_or_default();
        assert_eq!(public.len(), 2);
        assert!(is_public(&public));
        assert!(owner_only(
            &canned("bucket-owner-full-control", "me").unwrap_or_default(),
            "me"
        ));
        assert!(owner_only(
            &canned("private", "me").unwrap_or_default(),
            "me"
        ));
        assert!(!owner_only(
            &canned("authenticated-read", "me").unwrap_or_default(),
            "me"
        ));
        assert!(canned("nope", "me").is_none());
    }

    #[test]
    fn grant_headers_are_read_and_an_email_is_refused() {
        let g =
            header_grantees(r#"id="abc", uri="http://acs.amazonaws.com/groups/global/AllUsers""#)
                .unwrap_or_default();
        assert_eq!(
            g,
            [
                Grantee::User("abc".into()),
                Grantee::Group(ALL_USERS.into())
            ]
        );
        let e = header_grantees(r#"emailAddress="a@b.c""#)
            .err()
            .map(|e| e.code);
        assert_eq!(e.as_deref(), Some("UnresolvableGrantByEmailAddress"));
    }

    #[test]
    fn a_public_read_grant_becomes_a_statement_for_anyone() {
        let s = statements(
            &canned("public-read", "me").unwrap_or_default(),
            "arn:aws:s3:::b/k",
            false,
        );
        assert_eq!(s.len(), 1);
        assert_eq!(s[0]["Principal"], "*");
        assert_eq!(s[0]["Action"][0], "s3:GetObject");
        let w = statements(
            &canned("public-read-write", "me").unwrap_or_default(),
            "arn:aws:s3:::b",
            true,
        );
        assert_eq!(w[1]["Resource"][1], "arn:aws:s3:::b/*");
    }
}
