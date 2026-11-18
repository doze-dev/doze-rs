//! A function URL with `AuthType` `AWS_IAM`: what the gateway made of the signature decides, and the signer is who IAM asks.

use aws_peers::Mode;
use aws_wire::service::{Service, WebRequest, WebSignature};
use serde_json::{Value, json};

use super::behaviour::{TRUST, create_role, function_with_role};
use super::{World, have};

fn request(host: &str, signature: WebSignature) -> WebRequest {
    WebRequest {
        method: "GET".into(),
        host: host.into(),
        path: "/".into(),
        query: String::new(),
        headers: Vec::new(),
        body: bytes::Bytes::new(),
        endpoint: "http://127.0.0.1:4566".into(),
        signature,
    }
}

fn signed(key: &str, region: &str) -> WebSignature {
    WebSignature::Signed {
        access_key_id: key.into(),
        session_token: None,
        region: region.into(),
        verified: true,
    }
}

#[tokio::test]
async fn an_aws_iam_url_needs_a_good_signature_and_its_signer_is_asked() {
    if !have("python3") {
        return;
    }
    let w = World::with_mode(Mode::Enforce);
    let role = create_role(&w, "r", TRUST).await;
    let source = b"def handler(event, context):\n    return {'statusCode': 200, 'body': 'ran'}\n";
    function_with_role(&w, "iam", &role, source).await.unwrap();
    let url = w
        .ok(
            "CreateFunctionUrlConfig",
            json!({"FunctionName": "iam", "AuthType": "AWS_IAM"}),
        )
        .await;
    let host = url["FunctionUrl"]
        .as_str()
        .unwrap()
        .trim_start_matches("http://")
        .trim_end_matches('/')
        .to_owned();
    let web = w.lambda.web().unwrap();
    assert_eq!(web.signing_name(&host, "/"), Some("lambda"));
    let answer = |r: aws_wire::service::WebReply| {
        let body: Value = serde_json::from_slice(&r.body).unwrap_or(Value::Null);
        (
            r.status,
            // An unsigned request is refused under `Message`, a wrong signature under `message` (both recorded).
            body["Message"]
                .as_str()
                .or_else(|| body["message"].as_str())
                .unwrap_or_default()
                .to_owned(),
        )
    };
    // No signature: AWS's bare 403.
    let (status, message) = answer(web.serve(request(&host, WebSignature::Unsigned)).await);
    assert_eq!((status, message.as_str()), (403, "Forbidden"));
    // A signature the gateway refused: its words.
    let mismatch = "The request signature we calculated does not match the signature you provided.";
    let (status, message) = answer(
        web.serve(request(&host, WebSignature::Refused(mismatch.into())))
            .await,
    );
    assert_eq!((status, message.as_str()), (403, mismatch));
    // Scoped to another region.
    let (status, message) = answer(
        web.serve(request(&host, signed("AKIDLOCAL", "eu-west-1")))
            .await,
    );
    assert_eq!(status, 403);
    assert!(
        message.starts_with("Credential should be scoped to a valid region."),
        "{message}"
    );
    // A good signature by the account's own key: IAM lets the account in, and the function runs.
    let reply = web.serve(request(&host, signed("test", "us-east-1"))).await;
    assert_eq!(
        (reply.status, reply.body.as_ref()),
        (200, b"ran".as_slice())
    );
    w.stop().await;
}
