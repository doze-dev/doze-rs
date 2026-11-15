//! `AssumeRoleWithWebIdentity` end to end through the services: an identity provider on https (`aws_webhook::test_server`, whose
//! CA the tests trust) serves its discovery document and an ES256 key; a token it signed is accepted, and one with a changed claim, a
//! made-up key id, a wrong audience or a passed expiry is refused.

use std::sync::Arc;

use aws_testkit::Harness;
use aws_webhook::test_server::{Answerer, Ca, Server, serve};
use aws_wire::service::Service;
use doze_runtime::{Ctx, SystemClock, Tasks};
use p256::ecdsa::signature::Signer;
use p256::ecdsa::{Signature, SigningKey};
use serde_json::{Value, json};

use super::temp;
use crate::services;

fn b64url(bytes: &[u8]) -> String {
    aws_core::base64::encode(bytes)
        .replace('+', "-")
        .replace('/', "_")
        .trim_end_matches('=')
        .to_owned()
}

fn key() -> SigningKey {
    SigningKey::from_slice(&[7u8; 32]).unwrap()
}

/// The provider's public key as a JWK (`kid` "k1").
fn jwk() -> Value {
    let point = key().verifying_key().to_sec1_point(false);
    let raw = point.as_bytes();
    json!({"kty": "EC", "crv": "P-256", "kid": "k1", "use": "sig",
        "x": b64url(raw.get(1..33).unwrap()), "y": b64url(raw.get(33..65).unwrap())})
}

fn sign(header: &Value, claims: &Value) -> String {
    let signed = format!(
        "{}.{}",
        b64url(header.to_string().as_bytes()),
        b64url(claims.to_string().as_bytes())
    );
    let sig: Signature = key().sign(signed.as_bytes());
    format!("{signed}.{}", b64url(&sig.to_bytes()))
}

async fn issuer() -> (String, Server) {
    Ca::get().install();
    let port = Arc::new(std::sync::OnceLock::<u16>::new());
    let known = Arc::clone(&port);
    let answer: Answerer = Arc::new(move |req: &str| {
        let base = format!("https://localhost:{}", known.get().copied().unwrap_or(0));
        let doc = if req.starts_with("GET /.well-known/openid-configuration ") {
            json!({"issuer": base, "jwks_uri": format!("{base}/keys")})
        } else {
            json!({ "keys": [jwk()] })
        }
        .to_string();
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{doc}",
            doc.len()
        )
    });
    let server = serve(Ca::get().identity(), answer).await;
    port.set(server.port).unwrap();
    (format!("https://localhost:{}", server.port), server)
}

struct Both {
    iam: crate::Iam,
    sts: crate::Sts,
    build_iam: Harness<crate::Iam>,
    build_sts: Harness<crate::Sts>,
}

impl Both {
    /// The services over a fresh directory, and a second pair that only builds calls.
    fn start(dir: &aws_testkit::TempDir) -> Self {
        let ctx = |dir: std::path::PathBuf| Ctx {
            dir,
            idle: std::time::Duration::from_secs(300),
            tasks: Tasks::new(),
            clock: Arc::new(SystemClock),
            ext: None,
        };
        let (iam, sts) = services(Arc::new(ctx(dir.path().to_owned())));
        let (bi, bs) = services(Arc::new(ctx(
            std::env::temp_dir().join("doze-aws-iam-unused")
        )));
        Self {
            iam,
            sts,
            build_iam: Harness::new(bi),
            build_sts: Harness::new(bs),
        }
    }

    async fn iam(&self, op: &str, request: &Value) -> Result<Value, aws_wire::ApiError> {
        let ix = self.build_iam.op(op);
        let out = self.iam.call(self.build_iam.call(ix, request)?).await?;
        Ok(self.build_iam.json(ix, &out))
    }

    async fn assume(&self, token: &str) -> Result<Value, (String, String)> {
        let ix = self.build_sts.op("AssumeRoleWithWebIdentity");
        let request = json!({"RoleArn": "arn:aws:iam::000000000000:role/web", "RoleSessionName": "web", "WebIdentityToken": token});
        let call = self.build_sts.call(ix, &request).unwrap();
        match self.sts.call(call).await {
            Ok(out) => Ok(self.build_sts.json(ix, &out)),
            Err(e) => Err((
                e.shape
                    .map_or_else(|| e.code.to_string(), |s| s.to_string()),
                e.message,
            )),
        }
    }
}

#[tokio::test]
async fn a_token_is_accepted_only_when_its_provider_its_signature_its_audience_and_its_time_are_good()
 {
    let (url, _server) = issuer().await;
    let dir = temp();
    let w = Both::start(&dir);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let header = json!({"alg": "ES256", "kid": "k1", "typ": "JWT"});
    let claims = json!({"iss": url, "sub": "alice", "aud": "app", "exp": now + 600});
    let good = sign(&header, &claims);

    // No provider for the issuer yet: refused before anything is fetched.
    let (code, message) = w.assume(&good).await.unwrap_err();
    assert_eq!(code, "InvalidIdentityTokenException");
    assert!(
        message.starts_with("No OpenIDConnect provider found"),
        "{message}"
    );

    w.iam(
        "CreateOpenIDConnectProvider",
        &json!({"Url": url, "ClientIDList": ["app"]}),
    )
    .await
    .unwrap();
    let ok = w.assume(&good).await.unwrap();
    assert_eq!(ok["SubjectFromWebIdentityToken"], "alice");
    assert_eq!(ok["Audience"], "app");

    // The signature covers the claims: a changed subject does not verify.
    let mut parts: Vec<&str> = good.split('.').collect();
    let forged = b64url(
        json!({"iss": url, "sub": "mallory", "aud": "app", "exp": now + 600})
            .to_string()
            .as_bytes(),
    );
    parts[1] = &forged;
    let (code, message) = w.assume(&parts.join(".")).await.unwrap_err();
    assert_eq!(code, "InvalidIdentityTokenException");
    assert!(message.contains("signature"), "{message}");

    let other_kid = sign(&json!({"alg": "ES256", "kid": "k9"}), &claims);
    assert_eq!(
        w.assume(&other_kid).await.unwrap_err().0,
        "InvalidIdentityTokenException"
    );
    let unsigned = format!(
        "{}.{}.",
        b64url(br#"{"alg":"none"}"#),
        b64url(claims.to_string().as_bytes())
    );
    assert_eq!(
        w.assume(&unsigned).await.unwrap_err().0,
        "InvalidIdentityTokenException"
    );

    let wrong_aud = sign(
        &header,
        &json!({"iss": url, "sub": "alice", "aud": "other", "exp": now + 600}),
    );
    assert_eq!(
        w.assume(&wrong_aud).await.unwrap_err(),
        (
            "InvalidIdentityTokenException".to_owned(),
            "Incorrect token audience".to_owned()
        )
    );
    let expired = sign(
        &header,
        &json!({"iss": url, "sub": "alice", "aud": "app", "exp": now - 60}),
    );
    assert_eq!(
        w.assume(&expired).await.unwrap_err().0,
        "ExpiredTokenException"
    );
}
