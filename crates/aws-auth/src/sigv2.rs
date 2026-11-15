//! SigV2 presigned URLs, which boto3's `generate_presigned_url` still makes for S3 by default and S3 still accepts: the query carries
//! `AWSAccessKeyId`, `Expires` (an absolute time) and `Signature`, the base64 HMAC-SHA1 of
//!
//! ```text
//! VERB \n Content-MD5 \n Content-Type \n Expires \n x-amz-* headers (name:value\n, sorted) path-with-bucket ?sub-resources
//! ```
//!
//! (the S3 Developer Guide's "Signing and authenticating REST requests", and botocore's `HmacV1QueryAuth`). Only the query form is
//! checked; a SigV2 `Authorization` header stays refused.

use hmac::{Hmac, KeyInit, Mac};
use http::request::Parts;
use sha1::Sha1;

use crate::creds::Credentials;
use crate::error::AuthError;

/// The query parameters that are part of the resource signed (botocore's list; S3's sub-resources and response overrides).
const SUB_RESOURCES: &[&str] = &[
    "accelerate",
    "acl",
    "analytics",
    "cors",
    "delete",
    "inventory",
    "lifecycle",
    "location",
    "logging",
    "metrics",
    "notification",
    "object-lock",
    "partNumber",
    "policy",
    "replication",
    "requestPayment",
    "response-cache-control",
    "response-content-disposition",
    "response-content-encoding",
    "response-content-language",
    "response-content-type",
    "response-expires",
    "restore",
    "select",
    "select-type",
    "storageClass",
    "tagging",
    "torrent",
    "uploadId",
    "uploads",
    "versionId",
    "versioning",
    "versions",
    "website",
];

/// What a SigV2 presigned URL says about itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct V2 {
    pub access_key_id: String,
    /// Unix seconds after which the URL is refused.
    pub expires: i64,
    /// Base64, as sent.
    pub signature: String,
}

fn query(parts: &Parts) -> Vec<(String, String)> {
    form_urlencoded::parse(parts.uri.query().unwrap_or_default().as_bytes())
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect()
}

/// The claim of a SigV2 presigned URL, when the query is one.
#[must_use]
pub fn claim(parts: &Parts) -> Option<V2> {
    let q = query(parts);
    let get = |name: &str| q.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone());
    Some(V2 {
        access_key_id: get("AWSAccessKeyId")?,
        expires: get("Expires")?.parse().ok()?,
        signature: get("Signature")?,
    })
}

/// The string a SigV2 presigned URL signs. `resource_path` is the request path as sent, with the bucket in front when the request
/// named it in the host.
#[must_use]
pub fn string_to_sign(parts: &Parts, resource_path: &str, expires: i64) -> String {
    let header = |name: &str| {
        parts
            .headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .trim()
            .to_owned()
    };
    let q = query(parts);
    let mut amz: Vec<(String, String)> = parts
        .headers
        .iter()
        .filter(|(k, _)| k.as_str().starts_with("x-amz-"))
        .map(|(k, v)| {
            (
                k.as_str().to_owned(),
                v.to_str().unwrap_or_default().trim().to_owned(),
            )
        })
        .chain(
            q.iter()
                .filter(|(k, _)| k.to_ascii_lowercase().starts_with("x-amz-"))
                .map(|(k, v)| (k.to_ascii_lowercase(), v.clone())),
        )
        .collect();
    amz.sort();
    let mut sub: Vec<&(String, String)> = q
        .iter()
        .filter(|(k, _)| SUB_RESOURCES.contains(&k.as_str()))
        .collect();
    sub.sort();
    let mut out = format!(
        "{}\n{}\n{}\n{expires}\n",
        parts.method.as_str(),
        header("content-md5"),
        header("content-type")
    );
    for (k, v) in &amz {
        out.push_str(k);
        out.push(':');
        out.push_str(v);
        out.push('\n');
    }
    out.push_str(resource_path);
    for (i, (k, v)) in sub.iter().enumerate() {
        out.push(if i == 0 { '?' } else { '&' });
        out.push_str(k);
        if !v.is_empty() {
            out.push('=');
            out.push_str(v);
        }
    }
    out
}

/// The signature of `string_to_sign` under `secret`, base64 (what a client puts in `Signature`).
#[must_use]
pub fn sign(secret: &str, string_to_sign: &str) -> String {
    let Ok(mut mac) = <Hmac<Sha1> as KeyInit>::new_from_slice(secret.as_bytes()) else {
        return String::new();
    };
    mac.update(string_to_sign.as_bytes());
    aws_core::base64::encode(&mac.finalize().into_bytes())
}

/// Checks a SigV2 presigned URL at `now` (Unix seconds): its key, its token, its expiry and its signature. The access key id on
/// success.
///
/// # Errors
/// [`AuthError::Missing`] when the query is not a SigV2 URL; otherwise why it is refused, as for SigV4.
pub fn verify<C: Credentials>(
    parts: &Parts,
    resource_path: &str,
    now: i64,
    creds: &C,
) -> Result<String, AuthError> {
    let v2 = claim(parts).ok_or(AuthError::Missing)?;
    if now > v2.expires {
        return Err(AuthError::PresignExpired {
            request_time: v2.expires,
            expires_in: 0,
            server_time: now,
        });
    }
    let token = query(parts)
        .into_iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("x-amz-security-token"))
        .map(|(_, v)| v);
    let sts = string_to_sign(parts, resource_path, v2.expires);
    let claimed = aws_core::base64::decode(&v2.signature).unwrap_or_default();
    let good = creds.lookup(&v2.access_key_id, |c| {
        let c = c.ok_or_else(|| AuthError::UnknownAccessKey(v2.access_key_id.clone()))?;
        if c.expires_at.is_some_and(|t| t <= now) {
            return Err(AuthError::ExpiredToken);
        }
        if c.session_token.as_deref() != token.as_deref() {
            return Err(AuthError::InvalidToken);
        }
        let mut mac = <Hmac<Sha1> as KeyInit>::new_from_slice(c.secret.expose().as_bytes())
            .map_err(|_| AuthError::Internal)?;
        mac.update(sts.as_bytes());
        Ok(mac.verify_slice(&claimed).is_ok())
    })?;
    if good {
        return Ok(v2.access_key_id);
    }
    Err(AuthError::SignatureMismatch {
        access_key_id: v2.access_key_id,
        signature_provided: v2.signature,
        canonical_request: String::new(),
        string_to_sign: sts,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::creds::{Credential, Secret, StaticCredentials};

    fn parts(method: &str, uri: &str, headers: &[(&str, &str)]) -> Parts {
        let mut b = http::Request::builder().method(method).uri(uri);
        for (k, v) in headers {
            b = b.header(*k, *v);
        }
        b.body(()).unwrap().into_parts().0
    }

    fn creds() -> StaticCredentials {
        let mut c = StaticCredentials::new();
        c.insert(
            "test",
            Credential {
                secret: Secret::new("test".into()),
                session_token: None,
                expires_at: None,
            },
        );
        c
    }

    // Made by botocore 1.43 (`generate_presigned_url`, key `test`, secret `test`), with the string it signed.
    const PUT: &str =
        "/b/up?AWSAccessKeyId=test&Signature=MjPAW0vQejwsq60xay5Xea9G3R0%3D&Expires=1794648061";

    #[test]
    fn the_string_to_sign_is_botocores() {
        let p = parts(
            "GET",
            "/b/k?response-content-type=text%2Fx&versionId=v1&AWSAccessKeyId=a&Expires=9&Signature=s",
            &[],
        );
        assert_eq!(
            string_to_sign(&p, "/b/k", 9),
            "GET\n\n\n9\n/b/k?response-content-type=text/x&versionId=v1"
        );
        let p = parts(
            "PUT",
            "/b/up%20dir/x?AWSAccessKeyId=a",
            &[("x-amz-meta-a", " 1 ")],
        );
        assert_eq!(
            string_to_sign(&p, "/b/up%20dir/x", 5),
            "PUT\n\n\n5\nx-amz-meta-a:1\n/b/up%20dir/x"
        );
    }

    #[test]
    fn a_url_botocore_signed_verifies_until_it_expires_and_not_once_changed() {
        let p = parts("PUT", PUT, &[]);
        assert_eq!(
            verify(&p, "/b/up", 1_794_648_000, &creds()).unwrap(),
            "test"
        );
        assert!(matches!(
            verify(&p, "/b/up", 1_794_648_062, &creds()),
            Err(AuthError::PresignExpired { .. })
        ));
        // What urllib sends with a body: a Content-Type the URL did not sign (AWS refused it so, recorded 2026-11-09).
        let form = parts(
            "PUT",
            PUT,
            &[("content-type", "application/x-www-form-urlencoded")],
        );
        assert!(matches!(
            verify(&form, "/b/up", 1_794_648_000, &creds()),
            Err(AuthError::SignatureMismatch { .. })
        ));
        assert!(matches!(
            verify(
                &parts("PUT", PUT, &[]),
                "/b/up",
                1_794_648_000,
                &StaticCredentials::new()
            ),
            Err(AuthError::UnknownAccessKey(_))
        ));
    }
}
