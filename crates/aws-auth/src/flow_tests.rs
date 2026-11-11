//! The two-step check end to end: each way a request can fail before or at the signature, and a request that signs no payload
//! hash (the shape of every JSON, Query and CBOR request).
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use http::request::Parts;

use crate::canonical::{Flavor, canonical_request, string_to_sign};
use crate::creds::{Credential, Secret, StaticCredentials};
use crate::error::AuthError;
use crate::key::{SigningKey, sha256_hex};
use crate::verify::{Endpoint, begin};

const SECRET: &str = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";
const AKID: &str = "AKIAIOSFODNN7EXAMPLE";
/// 2013-05-24T00:00:00Z, the date of the documentation's examples.
const T0: i64 = 1_369_353_600;
const SQS: Endpoint = Endpoint {
    signing_name: "sqs",
    s3: false,
    max_skew_secs: 300,
};

fn creds(token: Option<&str>, expires_at: Option<i64>) -> StaticCredentials {
    let mut c = StaticCredentials::new();
    c.insert(
        AKID,
        Credential {
            secret: Secret::new(SECRET.to_owned()),
            session_token: token.map(str::to_owned),
            expires_at,
        },
    );
    c
}

/// A JSON-protocol request to `sqs`, signed with the same code the verifier uses to check it. (The documented known answers
/// prove that code against AWS's own examples; this proves the flow around it.)
fn signed_sqs(body: &str, token: Option<&str>, scope_service: &str) -> Parts {
    let headers = [
        ("host", "sqs.us-east-1.amazonaws.com"),
        ("x-amz-date", "20130524T000000Z"),
        ("x-amz-target", "AmazonSQS.ListQueues"),
    ];
    let signed: Vec<String> = ["host", "x-amz-date", "x-amz-target"]
        .map(String::from)
        .to_vec();
    let mut b = http::Request::builder().method("POST").uri("/");
    for (k, v) in headers {
        b = b.header(k, v);
    }
    if let Some(t) = token {
        b = b.header("x-amz-security-token", t);
    }
    let probe = b.body(()).unwrap().into_parts().0;
    let mut signed = signed;
    if token.is_some() {
        signed.push("x-amz-security-token".into());
        signed.sort();
    }
    let canonical = canonical_request(
        &probe,
        &signed,
        &sha256_hex(body.as_bytes()),
        Flavor {
            s3: false,
            presigned: false,
        },
    )
    .unwrap();
    let scope = format!("20130524/us-east-1/{scope_service}/aws4_request");
    let sts = string_to_sign("20130524T000000Z", &scope, &canonical);
    let key = SigningKey::derive(SECRET, "20130524", "us-east-1", scope_service).unwrap();
    let auth = format!(
        "AWS4-HMAC-SHA256 Credential={AKID}/{scope},SignedHeaders={},Signature={}",
        signed.join(";"),
        key.sign_hex(sts.as_bytes()).unwrap()
    );
    let (mut parts, ()) = http::Request::builder()
        .method("POST")
        .uri("/")
        .body(())
        .unwrap()
        .into_parts();
    parts.headers = probe.headers;
    parts.headers.insert("authorization", auth.parse().unwrap());
    parts
}

#[test]
fn a_request_with_no_signed_payload_hash_is_checked_once_the_body_hash_is_supplied() {
    let body = r#"{"QueueNamePrefix":"a"}"#;
    let parts = signed_sqs(body, None, "sqs");
    let pending = begin(&parts, T0, SQS, &creds(None, None)).unwrap();
    assert!(pending.needs_body_hash());
    assert_eq!(pending.claimed_service(), "sqs");
    let verified = pending
        .finish(&parts, Some(&sha256_hex(body.as_bytes())))
        .unwrap();
    assert_eq!(verified.access_key_id(), AKID);
    assert_eq!(
        (
            verified.scope().region.as_str(),
            verified.scope().service.as_str(),
            verified.scope().request_time
        ),
        ("us-east-1", "sqs", T0)
    );
}

#[test]
fn a_body_that_is_not_the_one_signed_fails_the_signature() {
    let parts = signed_sqs(r#"{"QueueNamePrefix":"a"}"#, None, "sqs");
    let pending = begin(&parts, T0, SQS, &creds(None, None)).unwrap();
    let wrong = sha256_hex(br#"{"QueueNamePrefix":"b"}"#);
    assert!(matches!(
        pending.finish(&parts, Some(&wrong)),
        Err(AuthError::SignatureMismatch { .. })
    ));
}

#[test]
fn finish_without_a_needed_hash_is_an_internal_fault_not_a_pass() {
    let parts = signed_sqs("{}", None, "sqs");
    let pending = begin(&parts, T0, SQS, &creds(None, None)).unwrap();
    assert!(matches!(
        pending.finish(&parts, None),
        Err(AuthError::Internal)
    ));
}

#[test]
fn what_the_head_can_settle_is_settled_before_the_body() {
    let parts = signed_sqs("{}", None, "sqs");
    let ok = creds(None, None);
    // No such key.
    assert!(
        matches!(begin(&parts, T0, SQS, &StaticCredentials::new()), Err(AuthError::UnknownAccessKey(k)) if k == AKID)
    );
    // A scope for another service than the endpoint's.
    let s3 = Endpoint {
        signing_name: "s3",
        ..SQS
    };
    assert!(matches!(
        begin(&parts, T0, s3, &ok),
        Err(AuthError::ScopeMismatch { .. })
    ));
    // The clock: 301 s out is refused, 300 s is not.
    assert!(matches!(
        begin(&parts, T0 + 301, SQS, &ok),
        Err(AuthError::Skewed { skew: 301, .. })
    ));
    assert!(begin(&parts, T0 + 300, SQS, &ok).is_ok());
    assert!(begin(&parts, T0 - 300, SQS, &ok).is_ok());
    assert!(matches!(
        begin(&parts, T0 - 301, SQS, &ok),
        Err(AuthError::Skewed { .. })
    ));
}

#[test]
fn session_credentials_need_their_token_and_their_time() {
    let with_token = signed_sqs("{}", Some("tok"), "sqs");
    let session = creds(Some("tok"), Some(T0 + 3600));
    assert!(begin(&with_token, T0, SQS, &session).is_ok());
    // Expired.
    assert!(matches!(
        begin(&with_token, T0 + 3600, SQS, &session),
        Err(AuthError::Skewed { .. } | AuthError::ExpiredToken)
    ));
    let expired = creds(Some("tok"), Some(T0));
    assert!(matches!(
        begin(&with_token, T0, SQS, &expired),
        Err(AuthError::ExpiredToken)
    ));
    // The wrong token, no token for a session key, and a token for a plain key.
    assert!(matches!(
        begin(&with_token, T0, SQS, &creds(Some("other"), None)),
        Err(AuthError::InvalidToken)
    ));
    let without = signed_sqs("{}", None, "sqs");
    assert!(matches!(
        begin(&without, T0, SQS, &session),
        Err(AuthError::InvalidToken)
    ));
    assert!(matches!(
        begin(&with_token, T0, SQS, &creds(None, None)),
        Err(AuthError::InvalidToken)
    ));
}

#[test]
fn an_unsigned_request_is_missing_not_malformed() {
    let (parts, ()) = http::Request::builder()
        .method("POST")
        .uri("/")
        .body(())
        .unwrap()
        .into_parts();
    assert!(matches!(
        begin(&parts, T0, SQS, &creds(None, None)),
        Err(AuthError::Missing)
    ));
}

#[test]
fn the_secret_is_never_printed() {
    let c = Credential {
        secret: Secret::new(SECRET.to_owned()),
        session_token: None,
        expires_at: None,
    };
    assert!(!format!("{c:?}").contains("wJalr"));
    let key = SigningKey::derive(SECRET, "20130524", "us-east-1", "sqs").unwrap();
    assert!(format!("{key:?}").contains("redacted"));
}
