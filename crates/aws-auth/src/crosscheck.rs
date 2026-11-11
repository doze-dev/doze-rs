//! What verifying with the `aws-sigv4` crate looks like. The crate is a *signer*: to verify, the
//! request is re-signed with the stored secret and the result compared. This module does that for
//! the documented S3 examples, to measure the API friction (see the notes on each step) next to
//! the hand-rolled path in `canonical.rs`/`key.rs`.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]
use crate::claim::{Claim, Placement};
use aws_credential_types::Credentials;
use aws_sigv4::http_request::{
    PayloadChecksumKind, PercentEncodingMode, SignableBody, SignableRequest, SignatureLocation,
    SigningSettings, UriPathNormalizationMode, sign,
};
use aws_sigv4::sign::v4;
use http::request::Parts;
use std::time::{Duration, UNIX_EPOCH};

const SECRET: &str = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";

/// The AWS crate's settings that make it sign the way the client said it did.
fn settings_for(claim: &Claim) -> SigningSettings {
    let mut settings = SigningSettings::default();
    // (1) The crate drops a built-in list of headers (user-agent, expect, ...) from signing; the
    //     verifier must sign exactly what the *client* listed, so the list has to be emptied.
    settings.excluded_headers = Some(vec![]);
    // (2) S3 flavour: no double encoding, no path normalisation, x-amz-content-sha256 is signed.
    settings.percent_encoding_mode = PercentEncodingMode::Single;
    settings.uri_path_normalization_mode = UriPathNormalizationMode::Disabled;
    if claim
        .signed_headers
        .iter()
        .any(|h| h == "x-amz-content-sha256")
    {
        settings.payload_checksum_kind = PayloadChecksumKind::XAmzSha256;
    }
    if claim.placement == Placement::Query {
        settings.signature_location = SignatureLocation::QueryParams;
        settings.expires_in = claim.expires_in.map(|s| Duration::from_secs(s as u64));
    }
    settings
}

/// Re-derive the signature for `parts` with the AWS crate.
fn resign(p: &Parts, claim: &Claim, payload_hash: &str) -> String {
    let identity = Credentials::new(
        &claim.access_key_id,
        SECRET,
        claim.session_token.clone(),
        None,
        "check",
    )
    .into();
    let settings = settings_for(claim);
    let params = v4::SigningParams::builder()
        .identity(&identity)
        .region(&claim.region)
        .name(&claim.service)
        .time(UNIX_EPOCH + Duration::from_secs(claim.request_time as u64))
        .settings(settings)
        .build()
        .unwrap()
        .into();

    // (3) The crate adds x-amz-date / x-amz-content-sha256 / x-amz-security-token itself, so those
    //     must be filtered out of the headers handed to it.
    let headers: Vec<(&str, &str)> = claim
        .signed_headers
        .iter()
        .filter(|h| {
            !matches!(
                h.as_str(),
                "x-amz-date" | "x-amz-content-sha256" | "x-amz-security-token"
            )
        })
        .map(|h| {
            (
                h.as_str(),
                p.headers.get(h.as_str()).unwrap().to_str().unwrap(),
            )
        })
        .collect();
    // (4) It wants a URI string; for presigned requests the X-Amz-* parameters (which it re-adds)
    //     must be stripped from the query first.
    let host = p.headers.get("host").unwrap().to_str().unwrap();
    let query: String = form_urlencoded::parse(p.uri.query().unwrap_or("").as_bytes())
        .filter(|(k, _)| !(claim.placement == Placement::Query && k.starts_with("X-Amz-")))
        .fold(
            form_urlencoded::Serializer::new(String::new()),
            |mut s, (k, v)| {
                s.append_pair(&k, &v);
                s
            },
        )
        .finish();
    let uri = if query.is_empty() {
        format!("https://{host}{}", p.uri.path())
    } else {
        format!("https://{host}{}?{query}", p.uri.path())
    };
    let body = if payload_hash == "UNSIGNED-PAYLOAD" {
        SignableBody::UnsignedPayload
    } else {
        SignableBody::Precomputed(payload_hash.to_owned())
    };
    let req = SignableRequest::new(p.method.as_str(), uri, headers.into_iter(), body).unwrap();
    // (5) It returns only the signature: no canonical request or string-to-sign for an error body.
    let (_instructions, signature) = sign(req, &params).unwrap().into_parts();
    signature
}

fn parts(method: &str, uri: &str, headers: &[(&str, &str)]) -> Parts {
    let mut b = http::Request::builder().method(method).uri(uri);
    for (k, v) in headers {
        b = b.header(*k, *v);
    }
    b.body(()).unwrap().into_parts().0
}

const EMPTY: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

#[test]
fn crate_reproduces_the_documented_signatures() {
    let auth = |signed: &str, sig: &str| {
        format!(
            "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/20130524/us-east-1/s3/aws4_request,SignedHeaders={signed},Signature={sig}"
        )
    };

    let a = auth(
        "host;range;x-amz-content-sha256;x-amz-date",
        "f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41",
    );
    let p = parts(
        "GET",
        "/test.txt",
        &[
            ("host", "examplebucket.s3.amazonaws.com"),
            ("range", "bytes=0-9"),
            ("x-amz-content-sha256", EMPTY),
            ("x-amz-date", "20130524T000000Z"),
            ("authorization", &a),
        ],
    );
    let claim = Claim::parse(&p).unwrap().unwrap();
    assert_eq!(
        resign(&p, &claim, EMPTY),
        "f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41"
    );

    let a = auth(
        "host;x-amz-content-sha256;x-amz-date",
        "fea454ca298b7da1c68078a5d1bdbfbbe0d65c699e0f91ac7a200a0136783543",
    );
    let p = parts(
        "GET",
        "/?lifecycle",
        &[
            ("host", "examplebucket.s3.amazonaws.com"),
            ("x-amz-content-sha256", EMPTY),
            ("x-amz-date", "20130524T000000Z"),
            ("authorization", &a),
        ],
    );
    let claim = Claim::parse(&p).unwrap().unwrap();
    assert_eq!(
        resign(&p, &claim, EMPTY),
        "fea454ca298b7da1c68078a5d1bdbfbbe0d65c699e0f91ac7a200a0136783543"
    );

    let p = parts(
        "GET",
        "/test.txt?X-Amz-Algorithm=AWS4-HMAC-SHA256&X-Amz-Credential=AKIAIOSFODNN7EXAMPLE%2F20130524%2Fus-east-1%2Fs3%2Faws4_request&X-Amz-Date=20130524T000000Z&X-Amz-Expires=86400&X-Amz-SignedHeaders=host&X-Amz-Signature=aeeed9bbccd4d02ee5c0109b86d86835f995330da4c265957d157751f604d404",
        &[("host", "examplebucket.s3.amazonaws.com")],
    );
    let claim = Claim::parse(&p).unwrap().unwrap();
    assert_eq!(
        resign(&p, &claim, "UNSIGNED-PAYLOAD"),
        "aeeed9bbccd4d02ee5c0109b86d86835f995330da4c265957d157751f604d404"
    );
}
