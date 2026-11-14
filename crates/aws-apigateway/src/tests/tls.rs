//! `https://` from the plane, against local TLS servers (`aws_webhook::test_server`, whose CA the tests trust): a JWT authorizer
//! whose issuer serves its keys over https, an HTTP API calling an https backend, and a REST API whose backend's certificate is
//! self-signed, refused unless the integration says `tlsConfig.insecureSkipVerification`.

use std::sync::Arc;

use aws_webhook::test_server::{Answerer, Ca, Identity, Server, ok, serve};
use rsa::pkcs8::DecodePrivateKey;
use rsa::signature::{SignatureEncoding, Signer};
use serde_json::{Value, json};

use super::{World, body, header};

/// The test key's modulus (`fixtures/jwt-rsa-test-key.pem`, a key made for these tests and nothing else), base64url.
const N: &str = "4SvjcBctKQTRtpqPa3GZTzDgm175lqan91HOzu0lFy6Yy9eDHPJkj8QR6wHVzkpugl7AgLY311UCL97uGN-96EQr5k0o-237H6W9jdxULvTmqXBTVh0WWPVMKbGxSdEQNWPUqS_2nzLmx1hNZSWvF4LpRosR67nPtlE84azNNA0CMsla_WhiZ4H8Fq1Yjy1iWZv1s-UH7wwjwkA59OaM0idBp345FJnjHJbGDiFlFrbJYezSMPUvQJ85na9NYGJBNjbvsU3PhBAHFgvsvhmKfO8SRVOCsXB4MtF2S0RpWG1li1nbPg4Ai3EoOfKQ_zns0YTwarNyqR_Qx1RmiwrmYw";
const KEY: &str = include_str!("fixtures/jwt-rsa-test-key.pem");

fn b64url(bytes: &[u8]) -> String {
    aws_core::base64::encode(bytes)
        .replace('+', "-")
        .replace('/', "_")
        .trim_end_matches('=')
        .to_owned()
}

/// An identity provider on https: its discovery document, its keys (`kid` "k1"), and tokens it signs.
pub(crate) struct Issuer {
    pub(crate) url: String,
    pub(crate) server: Server,
}

impl Issuer {
    pub(crate) async fn start(identity: Identity) -> Self {
        Ca::get().install();
        let port = Arc::new(std::sync::OnceLock::<u16>::new());
        let known = Arc::clone(&port);
        let answer: Answerer = Arc::new(move |req: &str| {
            let base = format!("https://localhost:{}", known.get().copied().unwrap_or(0));
            let doc = if req.starts_with("GET /.well-known/openid-configuration ") {
                json!({"issuer": base, "jwks_uri": format!("{base}/keys")})
            } else {
                json!({"keys": [{"kty": "RSA", "kid": "k1", "use": "sig", "alg": "RS256", "n": N, "e": "AQAB"}]})
            }
            .to_string();
            format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{doc}",
                doc.len()
            )
        });
        let server = serve(identity, answer).await;
        port.set(server.port).unwrap();
        Self {
            url: format!("https://localhost:{}", server.port),
            server,
        }
    }

    /// A token with `header` over `claims`, signed RS256 with the test key.
    pub(crate) fn sign_with(header: &Value, claims: &Value) -> String {
        let signed = format!(
            "{}.{}",
            b64url(header.to_string().as_bytes()),
            b64url(claims.to_string().as_bytes())
        );
        let key = rsa::PrivateKey::from_pkcs8_pem(KEY).unwrap();
        let signer = rsa::pkcs1v15::SigningKey::<rsa::sha2::Sha256>::new(key);
        let sig = signer.sign(signed.as_bytes()).to_vec();
        format!("Bearer {signed}.{}", b64url(&sig))
    }

    /// A good RS256 token (`kid` "k1").
    pub(crate) fn sign(claims: &Value) -> String {
        Self::sign_with(&json!({"alg": "RS256", "kid": "k1", "typ": "JWT"}), claims)
    }
}

/// An HTTP API with `GET /secret` behind a JWT authorizer for `issuer`, proxied to `backend`; returns its id.
async fn api_with_jwt(w: &World, issuer: &str, backend: &str) -> String {
    let api = w
        .http("CreateApi", json!({"name": "s", "protocolType": "HTTP"}))
        .await
        .unwrap();
    let id = api["apiId"].as_str().unwrap().to_owned();
    let integ = w
        .http("CreateIntegration", json!({"ApiId": id, "integrationType": "HTTP_PROXY", "integrationMethod": "GET", "integrationUri": format!("{backend}/inside"), "payloadFormatVersion": "1.0"}))
        .await
        .unwrap();
    let auth = w
        .http("CreateAuthorizer", json!({"ApiId": id, "name": "jwt", "authorizerType": "JWT", "identitySource": ["$request.header.Authorization"],
            "jwtConfiguration": {"issuer": issuer, "audience": ["me"]}}))
        .await
        .unwrap();
    w.http("CreateRoute", json!({"ApiId": id, "routeKey": "GET /secret", "authorizationType": "JWT", "authorizerId": auth["authorizerId"],
        "target": format!("integrations/{}", integ["integrationId"].as_str().unwrap())}))
        .await
        .unwrap();
    w.http(
        "CreateStage",
        json!({"ApiId": id, "stageName": "$default", "autoDeploy": true}),
    )
    .await
    .unwrap();
    id
}

async fn get_secret(w: &World, id: &str, token: &str) -> aws_wire::service::WebReply {
    w.web(
        "GET",
        &format!("/_aws/execute-api/{id}/secret"),
        "",
        &[("authorization", token)],
    )
    .await
}

fn why(r: &aws_wire::service::WebReply) -> String {
    header(r, "www-authenticate").unwrap_or_default().to_owned()
}

#[tokio::test]
async fn a_jwt_from_an_https_issuer_is_verified_and_the_backend_is_called_over_https() {
    let w = World::new();
    let issuer = Issuer::start(Ca::get().identity()).await;
    let backend = serve(
        Ca::get().identity(),
        Arc::new(|_: &str| "HTTP/1.1 200 OK\r\nContent-Length: 6\r\n\r\ninside".to_owned()),
    )
    .await;
    let id = api_with_jwt(
        &w,
        &issuer.url,
        &format!("https://localhost:{}", backend.port),
    )
    .await;
    let far = 4_000_000_000_i64;
    let claims = json!({"iss": issuer.url, "aud": "me", "exp": far, "sub": "ada"});

    let r = get_secret(&w, &id, &Issuer::sign(&claims)).await;
    assert_eq!(
        (r.status, &r.body[..]),
        (200, &b"inside"[..]),
        "{:?}",
        body(&r)
    );
    assert!(backend.seen(1).await[0].starts_with("GET /inside HTTP/1.1\r\n"));

    // A signature that does not verify, a key the issuer does not have, and a non-RSA algorithm are each refused, 401.
    let good = Issuer::sign(&claims);
    let flip = if good.as_bytes()[good.len() - 8] == b'A' {
        "B"
    } else {
        "A"
    };
    let tampered = format!(
        "{}{flip}{}",
        &good[..good.len() - 8],
        &good[good.len() - 7..]
    );
    let r = get_secret(&w, &id, &tampered).await;
    assert_eq!(
        (r.status, body(&r)),
        (401, json!({"message": "Unauthorized"}))
    );
    assert!(why(&r).contains("error=\"invalid_token\""), "{}", why(&r));
    let other_kid = Issuer::sign_with(&json!({"alg": "RS256", "kid": "k9"}), &claims);
    let r = get_secret(&w, &id, &other_kid).await;
    assert_eq!(r.status, 401);
    assert!(why(&r).contains("k9"), "{}", why(&r));
    let es = Issuer::sign_with(&json!({"alg": "ES256", "kid": "k1"}), &claims);
    let r = get_secret(&w, &id, &es).await;
    assert!(
        why(&r).contains("signing method ES256 is invalid"),
        "{}",
        why(&r)
    );
    let expired = Issuer::sign(&json!({"iss": issuer.url, "aud": "me", "exp": 1}));
    assert!(why(&get_secret(&w, &id, &expired).await).contains("the token has expired"));

    // The keys were fetched once (discovery document and key set) and kept: no fetch per request, nor for the unknown kid.
    assert_eq!(issuer.server.seen(2).await.len(), 2);
}

#[tokio::test]
async fn an_issuer_whose_certificate_is_not_trusted_cannot_vouch_for_a_token() {
    let w = World::new();
    let issuer = Issuer::start(Identity::self_signed()).await;
    let id = api_with_jwt(&w, &issuer.url, "http://127.0.0.1:9").await;
    let claims = json!({"iss": issuer.url, "aud": "me", "exp": 4_000_000_000_i64});
    let r = get_secret(&w, &id, &Issuer::sign(&claims)).await;
    assert_eq!(r.status, 401);
    assert!(why(&r).contains("unable to fetch"), "{}", why(&r));
    assert!(issuer.server.seen(0).await.is_empty());
}

#[tokio::test]
async fn a_self_signed_backend_needs_insecure_skip_verification() {
    let w = World::new();
    Ca::get().install();
    let backend = serve(Identity::self_signed(), ok()).await;
    let (api, rid) = w.rest_api_with("up").await;
    let put = |tls: Value| {
        let mut p = json!({"restApiId": api, "resourceId": rid, "requestHttpMethod": "GET", "type": "HTTP_PROXY",
            "httpMethod": "GET", "uri": format!("https://localhost:{}/x", backend.port)});
        if !tls.is_null() {
            p["tlsConfig"] = tls;
        }
        p
    };
    w.rest("PutIntegration", put(Value::Null)).await.unwrap();
    w.rest(
        "CreateDeployment",
        json!({"restApiId": api, "stageName": "s"}),
    )
    .await
    .unwrap();
    let path = format!("/_aws/execute-api/{api}/s/up");
    let r = w.web("GET", &path, "", &[]).await;
    assert_eq!(
        (r.status, body(&r)),
        (500, json!({"message": "Internal server error"}))
    );
    assert!(backend.seen(0).await.is_empty());

    w.rest(
        "PutIntegration",
        put(json!({"insecureSkipVerification": true})),
    )
    .await
    .unwrap();
    w.rest(
        "CreateDeployment",
        json!({"restApiId": api, "stageName": "s"}),
    )
    .await
    .unwrap();
    assert_eq!(w.web("GET", &path, "", &[]).await.status, 200);
    assert_eq!(backend.seen(1).await.len(), 1);
}
