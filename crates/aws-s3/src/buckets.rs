//! Buckets: make, remove, look at, list, and say where one is.

use aws_wire::ApiError;
use serde_json::json;

use crate::acl::Ownership;
use crate::core::{Core, HOME};
use crate::errors::{already_owned, bucket_not_empty, invalid_bucket_name, no_such_bucket};
use crate::model::Bucket;
use crate::req::{Answer, Out, Req};

/// Whether `name` follows the bucket naming rules (3 to 63 characters of lower-case letters, digits, hyphens and dots; starts and
/// ends with a letter or digit; no `..`; not an IP address; none of the reserved prefixes and suffixes).
pub(crate) fn valid_name(name: &str) -> bool {
    let ok_chars = name
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.');
    let edge = |c: Option<char>| c.is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    (3..=63).contains(&name.len())
        && ok_chars
        && edge(name.chars().next())
        && edge(name.chars().last())
        && !name.contains("..")
        && name.parse::<std::net::Ipv4Addr>().is_err()
        && !["xn--", "sthree-", "amzn-s3-demo-"]
            .iter()
            .any(|p| name.starts_with(p))
        && !["-s3alias", "--ol-s3", ".mrap", "--x-s3"]
            .iter()
            .any(|s| name.ends_with(s))
}

/// The owner every bucket and object answers with: one local account.
pub(crate) fn owner(account: &str) -> serde_json::Value {
    let id = account.repeat(6);
    json!({"DisplayName": "doze", "ID": id.get(..64).unwrap_or(&id)})
}

impl Core {
    pub(crate) fn create_bucket(&mut self, req: &Req<'_>) -> Answer {
        let name = req.bucket();
        if !valid_name(name) {
            return Err(invalid_bucket_name(name));
        }
        let region = req.region();
        let constraint = req
            .get("CreateBucketConfiguration")
            .and_then(|c| c.get("LocationConstraint"))
            .and_then(serde_json::Value::as_str);
        check_location(region, constraint)?;
        if self.buckets.get(name).is_some() {
            if region == HOME {
                return Self::created(name);
            }
            return Err(already_owned(name));
        }
        let created_ms = self.now_ms();
        let object_lock = req.flag("ObjectLockEnabledForBucket").unwrap_or(false);
        let named = req.s("ObjectOwnership");
        let ownership = named
            .and_then(Ownership::parse)
            .unwrap_or(Ownership::Enforced);
        let acl = Self::create_bucket_acl(req, ownership)?;
        let mut configs = std::collections::BTreeMap::new();
        if let Some(named) = named {
            configs.insert(
                "ownership".to_owned(),
                json!({"Rules": [{"ObjectOwnership": named}]}).to_string(),
            );
        }
        if let Some(acl) = acl {
            let text =
                serde_json::to_string(&acl).map_err(|e| ApiError::internal(e.to_string()))?;
            configs.insert("acl".to_owned(), text);
        }
        self.buckets.insert(
            name.to_owned(),
            Bucket {
                created_ms,
                object_lock,
                configs,
                ..Bucket::default()
            },
        );
        if object_lock {
            self.set_versioning(name, crate::model::Versioning::Enabled);
        }
        Self::created(name)
    }

    fn created(name: &str) -> Answer {
        Out::json(
            json!({"Location": format!("/{name}"), "BucketArn": format!("arn:aws:s3:::{name}")}),
        )
    }

    pub(crate) fn delete_bucket(&mut self, req: &Req<'_>) -> Answer {
        let name = req.bucket();
        self.bucket(name)?;
        if self.has_objects(name)? {
            return Err(bucket_not_empty(name));
        }
        self.buckets.remove(name);
        self.sync_marker();
        let gone: Vec<String> = self
            .uploads
            .with_prefix(&format!("{name}\0"))
            .map(|(k, _)| k.to_owned())
            .collect();
        for k in gone {
            self.remove_upload(&k);
        }
        Out::empty()
    }

    pub(crate) fn head_bucket(&self, req: &Req<'_>) -> Answer {
        self.bucket(req.bucket())?;
        Out::json(json!({"BucketRegion": req.region()}))
    }

    pub(crate) fn list_buckets(&self, req: &Req<'_>) -> Answer {
        let prefix = req.s("Prefix").unwrap_or_default();
        let buckets: Vec<_> = self
            .buckets
            .iter()
            .filter(|(name, _)| name.starts_with(prefix))
            .map(|(name, b)| {
                json!({"Name": name, "CreationDate": b.created_ms / 1000, "BucketRegion": req.region()})
            })
            .collect();
        Out::json(json!({"Buckets": buckets, "Owner": owner(req.account())}))
    }

    pub(crate) fn get_bucket_location(&self, req: &Req<'_>) -> Answer {
        self.bucket(req.bucket())?;
        if req.region() == HOME {
            return Out::empty();
        }
        Out::json(json!({"LocationConstraint": req.region()}))
    }

    /// The bucket, or the error AWS gives for the bucket an operation on a missing one.
    pub(crate) fn require(&self, name: &str) -> Result<&Bucket, ApiError> {
        self.buckets.get(name).ok_or_else(|| no_such_bucket(name))
    }
}

/// The location constraint must be the region the request was sent to (none is the home region's).
fn check_location(region: &str, constraint: Option<&str>) -> Result<(), ApiError> {
    match (region == HOME, constraint) {
        (true, None) => Ok(()),
        (false, Some(c)) if c == region => Ok(()),
        (false, None) => Err(ApiError::new(
            400,
            "IllegalLocationConstraintException",
            "The unspecified location constraint is incompatible for the region specific endpoint this request was sent to.",
        )),
        // (memory) a stated constraint that is not the region.
        _ => Err(ApiError::new(
            400,
            "InvalidLocationConstraint",
            "The specified location-constraint is not valid",
        )
        .with("LocationConstraint", constraint.unwrap_or_default())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bucket_names_follow_the_rules() {
        for good in ["abc", "my-bucket.logs", "a1b", &"a".repeat(63)] {
            assert!(valid_name(good), "{good}");
        }
        for bad in [
            "ab",
            "Not_A_Bucket",
            "-abc",
            "abc-",
            "a..b",
            "192.168.1.1",
            "xn--abc",
            "abc-s3alias",
            &"a".repeat(64),
            "UPPER",
        ] {
            assert!(!valid_name(bad), "{bad}");
        }
    }
}
