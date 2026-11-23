//! `POST /api/v1/resources/s3/buckets/<name>/presign`: a link to an object that downloads it without signing in, until it
//! expires. SigV4's query form, signed here (the console holds no person's key): `{"key": "a/b.jpg", "expires": 3600,
//! "version_id": null}` (`expires` in seconds, 1 to 12 hours, as the AWS console limits it) answers `{url, expires_at,
//! expires_in, verified}`.
//!
//! An upload's properties (`?props=`, JSON) are the other thing here: the members of PutObject an upload may set besides the key and
//! the type (see [`put_props`]).
//!
//! The object is read first (HeadObject, as the caller `console`), so a link is made only to what is there. The signature is
//! made with a key the console invents, which a doze-aws that trusts every key accepts (it judges the link's expiry); one that
//! verifies signatures (`--auth verify`) refuses it, and `verified` says so, so the page can tell the person before they copy it.

use std::time::{SystemTime, UNIX_EPOCH};

use aws_auth::canonical::string_to_sign;
use aws_auth::key::SigningKey;
use aws_core::{time, uri};
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use super::resources::{find, refused};
use super::{error, json};
use crate::resources::path;
use crate::state::Shared;

/// The longest a link lives through the console: 12 hours.
pub const MOST: u64 = 12 * 3600;
/// The access key the console signs with (it is no one's: a trusting gateway takes any).
const ACCESS_KEY: &str = "AKIADOZECONSOLE00000";
const SECRET: &str = "doze-console-presign";

#[derive(Deserialize)]
struct Ask {
    key: String,
    expires: Option<u64>,
    version_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Presigned {
    pub url: String,
    /// When it stops working, in milliseconds since 1970.
    pub expires_at: u64,
    pub expires_in: u64,
    /// Whether this doze-aws checks signatures: when it does, the link made here is refused (the console holds no issued key).
    pub verified: bool,
}

/// The signed address of `key` of `bucket` at `endpoint`, valid from `now` (seconds since 1970) for `expires` seconds, signed for
/// `region`. `version_id` names one version.
#[must_use]
pub fn sign(
    endpoint: &str,
    (bucket, key, version_id): (&str, &str, Option<&str>),
    (region, now, expires): (&str, i64, u64),
) -> String {
    let endpoint = endpoint.trim_end_matches('/');
    let host = endpoint.split_once("://").map_or(endpoint, |(_, h)| h);
    let amz = time::format_amz_date(now);
    let date = amz.get(..8).unwrap_or_default();
    let scope = format!("{date}/{region}/s3/aws4_request");
    let enc = |s: &str| uri::encode(s.as_bytes(), false);
    let mut query: Vec<(String, String)> = vec![
        ("X-Amz-Algorithm".into(), "AWS4-HMAC-SHA256".into()),
        (
            "X-Amz-Credential".into(),
            enc(&format!("{ACCESS_KEY}/{scope}")),
        ),
        ("X-Amz-Date".into(), amz.clone()),
        ("X-Amz-Expires".into(), expires.to_string()),
        ("X-Amz-SignedHeaders".into(), "host".into()),
    ];
    if let Some(v) = version_id {
        query.push(("versionId".into(), enc(v)));
    }
    query.sort();
    let query = query
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&");
    let address = uri::encode(format!("/{bucket}/{key}").as_bytes(), true);
    let canonical = format!("GET\n{address}\n{query}\nhost:{host}\n\nhost\nUNSIGNED-PAYLOAD");
    let signature = SigningKey::derive(SECRET, date, region, "s3")
        .and_then(|k| k.sign_hex(string_to_sign(&amz, &scope, &canonical).as_bytes()))
        .unwrap_or_default();
    format!("{endpoint}{address}?{query}&X-Amz-Signature={signature}")
}

/// The members of PutObject an upload may set beyond what the console always sends (the bucket, the key, the length, the type):
/// the properties of the board's Upload page. Anything else in `props` is refused, never passed through.
const PUT_MEMBERS: &[&str] = &[
    "Metadata",
    "Tagging",
    "StorageClass",
    "ServerSideEncryption",
    "SSEKMSKeyId",
    "BucketKeyEnabled",
    "ChecksumAlgorithm",
    "CacheControl",
    "ContentDisposition",
    "ContentEncoding",
    "ContentLanguage",
];

/// The most a `props` text may be (metadata may be 2 KB; tags 10 of 128 and 256 characters).
const PROPS_MAX: usize = 16 * 1024;

/// The members `?props=` of an upload sets: a JSON object of the members an upload may set (`PUT_MEMBERS`).
///
/// # Errors
/// A message that says what is wrong with `props`: not an object, too long, or a member the upload may not set.
pub fn put_props(
    query: Option<&str>,
    decode: impl Fn(&str) -> String,
) -> Result<Map<String, Value>, String> {
    let Some(text) = super::param(query, "props").map(decode) else {
        return Ok(Map::new());
    };
    if text.len() > PROPS_MAX {
        return Err(format!("The properties are over {PROPS_MAX} bytes."));
    }
    let Ok(Value::Object(props)) = serde_json::from_str::<Value>(&text) else {
        return Err("The properties are a JSON object.".to_owned());
    };
    if let Some(bad) = props.keys().find(|k| !PUT_MEMBERS.contains(&k.as_str())) {
        return Err(format!("An upload does not set {bad}."));
    }
    Ok(props)
}

/// `POST /api/v1/resources/s3/buckets/<name>/presign`.
pub async fn presign(
    State(s): State<Shared>,
    Path((service, kind, name)): Path<(String, String, String)>,
    body: Bytes,
) -> Response {
    if (service.as_str(), kind.as_str()) != ("s3", "buckets") {
        return error(
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            "Only an object of a bucket has a presigned link.",
        );
    }
    let Ok(ask) = serde_json::from_slice::<Ask>(&body) else {
        return error(
            StatusCode::BAD_REQUEST,
            "BAD_REQUEST",
            "Name the object (`key`) and, if you like, `expires` in seconds.",
        );
    };
    let expires = ask.expires.unwrap_or(3600);
    if ask.key.is_empty() || !(1..=MOST).contains(&expires) {
        return error(
            StatusCode::BAD_REQUEST,
            "BAD_REQUEST",
            "A link lives from 1 second to 12 hours, and names an object.",
        );
    }
    let (bus, d, _) = match find(&s, &service, &kind) {
        Ok(found) => found,
        Err(res) => return *res,
    };
    let mut head = json!({ "Bucket": name, "Key": ask.key });
    if let (Some(v), Some(o)) = (&ask.version_id, head.as_object_mut()) {
        o.insert("VersionId".to_owned(), Value::String(v.clone()));
    }
    if let Err(e) = bus.call_rest(d.target, "HeadObject", &head, None).await {
        return refused(&e);
    }
    let region = match bus
        .call(d.target, "GetBucketLocation", &json!({ "Bucket": name }))
        .await
    {
        Ok(v) => path::text(&v, "LocationConstraint").unwrap_or_default(),
        Err(e) => return refused(&e),
    };
    let region = if region.is_empty() {
        "us-east-1"
    } else {
        &region
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0));
    let url = sign(
        &s.facts.endpoint,
        (&name, &ask.key, ask.version_id.as_deref()),
        (region, now, expires),
    );
    json(
        StatusCode::OK,
        &Presigned {
            url,
            expires_at: u64::try_from(now).unwrap_or(0).saturating_add(expires) * 1000,
            expires_in: expires,
            verified: s.facts.auth == "verify",
        },
    )
}

#[cfg(test)]
mod tests {
    use aws_auth::canonical::{Flavor, canonical_request};
    use axum::http::Request;

    use super::*;

    #[test]
    fn a_link_verifies_as_sigv4_checks_a_presigned_get() {
        let url = sign(
            "http://127.0.0.1:4566",
            ("product-images", "banners/hero image.jpg", Some("v 1")),
            ("us-east-1", 1_794_000_000, 3600),
        );
        let (_, rest) = url.split_once("://").unwrap();
        let (host, target) = rest.split_once('/').unwrap();
        // The request a gateway would see, and the canonical request it builds of it.
        let req = Request::builder()
            .uri(format!("/{target}"))
            .header("host", host)
            .body(())
            .unwrap();
        let (parts, ()) = req.into_parts();
        let canonical = canonical_request(
            &parts,
            &["host".to_owned()],
            "UNSIGNED-PAYLOAD",
            Flavor {
                s3: true,
                presigned: true,
            },
        )
        .unwrap();
        let amz = time::format_amz_date(1_794_000_000);
        let date = &amz[..8];
        let scope = format!("{date}/us-east-1/s3/aws4_request");
        let key = SigningKey::derive(SECRET, date, "us-east-1", "s3").unwrap();
        let claimed = url.rsplit_once("X-Amz-Signature=").unwrap().1;
        assert!(key.verify_hex(string_to_sign(&amz, &scope, &canonical).as_bytes(), claimed));
        // What the person copies names the object and the version, and says how long it lives.
        assert!(url.starts_with("http://127.0.0.1:4566/product-images/banners/hero%20image.jpg?"));
        assert!(url.contains("versionId=v%201") && url.contains("X-Amz-Expires=3600"));
        assert!(url.contains(&format!(
            "X-Amz-Credential=AKIADOZECONSOLE00000%2F{date}%2Fus-east-1%2Fs3%2Faws4_request"
        )));
    }
}
