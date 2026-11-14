//! Access control lists, as S3 has had them since April 2023: a bucket's Object Ownership decides whether they mean anything.
//!
//! - `BucketOwnerEnforced` (every new bucket unless its creator says otherwise): ACLs are disabled. A read answers the owner with
//!   `FULL_CONTROL`; a write that grants anything more is refused with `AccessControlListNotSupported`. Only the owner's full control
//!   is accepted (`bucket-owner-full-control`, or `private`, or the same grant spelled out), and for a new bucket only `private`.
//! - `BucketOwnerPreferred` and `ObjectWriter`: ACLs are kept and returned, and an `AllUsers` grant makes the bucket or object
//!   readable (or writable) by anyone, unless the bucket's public access block ignores public ACLs. `BlockPublicAcls` refuses a
//!   write that would make one public.
//!
//! The bucket's ACL is kept with its configurations (`acl`), an object's with its version.
use aws_wire::ApiError;
use serde_json::{Value, json};

use crate::core::Core;
pub(crate) use crate::grants::{Grant, Ownership, owner_id, owner_only};
use crate::grants::{describe, is_public, requested, statements};
use crate::req::{Answer, Out, Req};

/// A request that sets an ACL on a bucket whose ACLs are disabled. (memory) AWS's words.
fn not_supported() -> ApiError {
    ApiError::new(
        400,
        "AccessControlListNotSupported",
        "The bucket does not allow ACLs",
    )
}

/// A public ACL on a bucket whose public access block blocks them. (memory)
fn public_blocked() -> ApiError {
    ApiError::new(403, "AccessDenied", "Access Denied")
}

impl Core {
    /// The bucket's Object Ownership: as configured; `ObjectWriter` once its ownership controls were deleted; otherwise
    /// `BucketOwnerEnforced`, every new bucket's.
    pub(crate) fn ownership(&self, bucket: &str) -> Ownership {
        let Some(b) = self.buckets.get(bucket) else {
            return Ownership::Enforced;
        };
        if b.configs.contains_key(&crate::config::deleted("ownership")) {
            return Ownership::Writer;
        }
        b.configs
            .get("ownership")
            .and_then(|c| serde_json::from_str::<Value>(c).ok())
            .and_then(|c| {
                c.pointer("/Rules/0/ObjectOwnership")?
                    .as_str()
                    .and_then(Ownership::parse)
            })
            .unwrap_or(Ownership::Enforced)
    }

    /// One setting of the bucket's public access block: all on for a new bucket, all off once the block was deleted.
    pub(crate) fn blocks(&self, bucket: &str, setting: &str) -> bool {
        let Some(b) = self.buckets.get(bucket) else {
            return true;
        };
        match b.configs.get("public-access-block") {
            Some(c) => serde_json::from_str::<Value>(c)
                .is_ok_and(|c| c.get(setting).and_then(Value::as_bool) == Some(true)),
            None => !b
                .configs
                .contains_key(&crate::config::deleted("public-access-block")),
        }
    }

    /// The ACL an object write (`PutObject`, `CopyObject`, `CreateMultipartUpload`) asks for, checked against the bucket: refused
    /// when ACLs are disabled and it grants more than the owner's full control, or when it is public and public ACLs are blocked.
    /// What to keep: nothing when ACLs are disabled or none was asked for.
    pub(crate) fn object_write_acl(&self, req: &Req<'_>) -> Result<Option<Vec<Grant>>, ApiError> {
        let owner = owner_id(req.account());
        let Some(grants) = requested(req, &owner)? else {
            return Ok(None);
        };
        self.check_acl_write(req.bucket(), &grants, &owner, false)
    }

    fn check_acl_write(
        &self,
        bucket: &str,
        grants: &[Grant],
        owner: &str,
        on_bucket: bool,
    ) -> Result<Option<Vec<Grant>>, ApiError> {
        if self.ownership(bucket) == Ownership::Enforced {
            if on_bucket || !owner_only(grants, owner) {
                return Err(not_supported());
            }
            return Ok(None);
        }
        if is_public(grants) && self.blocks(bucket, "BlockPublicAcls") {
            return Err(public_blocked());
        }
        Ok(Some(grants.to_vec()))
    }

    /// The ACL a new bucket is made with: under `BucketOwnerEnforced` (the default) only `private` or the owner's own grant.
    pub(crate) fn create_bucket_acl(
        req: &Req<'_>,
        ownership: Ownership,
    ) -> Result<Option<Vec<Grant>>, ApiError> {
        let owner = owner_id(req.account());
        let Some(grants) = requested(req, &owner)? else {
            return Ok(None);
        };
        if ownership == Ownership::Enforced && !owner_only(&grants, &owner) {
            // (memory) AWS's words for an ACL on a new bucket whose ACLs are disabled.
            return Err(ApiError::new(
                400,
                "InvalidBucketAclWithObjectOwnership",
                "Bucket cannot have ACLs set with ObjectOwnership's BucketOwnerEnforced setting",
            ));
        }
        Ok((ownership != Ownership::Enforced).then_some(grants))
    }

    /// The bucket's ACL, when it has one other than the owner's full control and ACLs are enabled.
    pub(crate) fn bucket_acl(&self, bucket: &str) -> Option<Vec<Grant>> {
        if self.ownership(bucket) == Ownership::Enforced {
            return None;
        }
        self.buckets
            .get(bucket)
            .and_then(|b| b.configs.get("acl"))
            .and_then(|text| serde_json::from_str(text).ok())
    }

    pub(crate) fn get_bucket_acl(&self, req: &Req<'_>) -> Answer {
        self.require(req.bucket())?;
        Out::json(describe(
            self.bucket_acl(req.bucket()).as_deref(),
            req.account(),
        ))
    }

    pub(crate) fn put_bucket_acl(&mut self, req: &Req<'_>) -> Answer {
        self.require(req.bucket())?;
        let owner = owner_id(req.account());
        let grants = requested(req, &owner)?.ok_or_else(missing_acl)?;
        let kept = self.check_acl_write(req.bucket(), &grants, &owner, true)?;
        let text = serde_json::to_string(&kept).map_err(|e| ApiError::internal(e.to_string()))?;
        if let Some(b) = self.buckets.get_mut(req.bucket()) {
            b.configs.insert("acl".into(), text);
        }
        Out::empty()
    }

    pub(crate) fn get_object_acl(&self, req: &Req<'_>) -> Answer {
        let v = self.readable(req.bucket(), req.key(), req.s("VersionId"))?;
        let enforced = self.ownership(req.bucket()) == Ownership::Enforced;
        let grants = if enforced { None } else { v.acl.as_deref() };
        Out::json(describe(grants, req.account()))
    }

    pub(crate) fn put_object_acl(&mut self, req: &Req<'_>) -> Answer {
        let mut v = self.readable(req.bucket(), req.key(), req.s("VersionId"))?;
        let owner = owner_id(req.account());
        let grants = requested(req, &owner)?.ok_or_else(missing_acl)?;
        if let Some(kept) = self.check_acl_write(req.bucket(), &grants, &owner, false)? {
            v.acl = Some(kept);
            self.put_row(req.bucket(), &v)?;
        }
        Out::empty()
    }
}

/// `policy` with `extra` statements added (either may be missing).
fn merged(policy: Option<String>, extra: Vec<Value>) -> Option<String> {
    if extra.is_empty() {
        return policy;
    }
    let mut doc: Value = policy
        .as_deref()
        .and_then(|p| serde_json::from_str(p).ok())
        .unwrap_or_else(|| json!({"Version": "2012-10-17"}));
    let mut statements = match doc.get("Statement") {
        Some(Value::Array(a)) => a.clone(),
        Some(one @ Value::Object(_)) => vec![one.clone()],
        _ => Vec::new(),
    };
    statements.extend(extra);
    if let Some(o) = doc.as_object_mut() {
        o.insert("Statement".into(), Value::Array(statements));
    }
    Some(doc.to_string())
}

impl Core {
    /// The resource policy the authorizer weighs for a request to `bucket` (and `key`): the bucket policy; for an anonymous caller,
    /// with what public ACLs grant added (unless ACLs are disabled or the public access block ignores them), and without a public
    /// policy while the block restricts public buckets (AWS then lets only the owner's account and AWS services in).
    pub(crate) fn resource_policy(
        &self,
        anonymous: bool,
        bucket: &str,
        key: Option<(&str, Option<&str>)>,
    ) -> Option<String> {
        let policy = self
            .buckets
            .get(bucket)
            .and_then(|b| b.configs.get("policy"))
            .cloned();
        if !anonymous {
            return policy;
        }
        let restricted = self.blocks(bucket, "RestrictPublicBuckets");
        let policy = policy.filter(|p| {
            !restricted
                || !serde_json::from_str::<Value>(p).is_ok_and(|d| crate::public::is_public(&d))
        });
        if self.ownership(bucket) == Ownership::Enforced || self.blocks(bucket, "IgnorePublicAcls")
        {
            return policy;
        }
        let arn = format!("arn:aws:s3:::{bucket}");
        let mut extra = self
            .bucket_acl(bucket)
            .map(|g| statements(&g, &arn, true))
            .unwrap_or_default();
        if let Some((key, version)) = key {
            let v = match version {
                Some(id) => self.find(bucket, key, id).ok().flatten(),
                None => self.latest(bucket, key).ok().flatten(),
            };
            if let Some(grants) = v.and_then(|v| v.acl) {
                extra.extend(statements(&grants, &format!("{arn}/{key}"), false));
            }
        }
        merged(policy, extra)
    }
}

/// A `Put*Acl` that names no ACL at all. (memory)
fn missing_acl() -> ApiError {
    ApiError::new(
        400,
        "MissingSecurityHeader",
        "Your request was missing a required header",
    )
    .with("MissingHeaderName", "x-amz-acl")
}
