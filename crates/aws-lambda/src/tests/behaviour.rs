//! Running as a role, asynchronous invocation and its destinations, reserved concurrency, response streaming, function URLs, and
//! code run in place.

use aws_core::base64;
use aws_peers::{Mode, Need};
use aws_wire::rest::RestReply;
use aws_wire::service::{Service, Web, WebRequest};
use serde_json::{Value, json};

use super::{ACCOUNT, World, have};

const TRUST: &str = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":{"Service":"lambda.amazonaws.com"},"Action":"sts:AssumeRole"}]}"#;
const SEND_ONLY: &str = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Action":"sqs:SendMessage","Resource":"*"}]}"#;

const WHOAMI: &[u8] = b"import os\ndef handler(event, context):\n    return {'key': os.environ['AWS_ACCESS_KEY_ID'], 'token': os.environ['AWS_SESSION_TOKEN']}\n";
const FAILS: &[u8] = b"def handler(event, context):\n    raise RuntimeError('no')\n";
const WORKS: &[u8] = b"def handler(event, context):\n    return {'ok': event}\n";

async fn create_role(w: &World, name: &str, trust: &str) -> String {
    let made = w
        .peer(
            "iam",
            "CreateRole",
            json!({"RoleName": name, "AssumeRolePolicyDocument": trust}),
        )
        .await;
    made["Role"]["Arn"].as_str().unwrap().to_owned()
}

async fn function_with_role(
    w: &World,
    name: &str,
    role: &str,
    source: &[u8],
) -> Result<Value, aws_wire::ApiError> {
    let zip = crate::zipread::build(&[("index.py", source, 0o644)]);
    w.call(
        "CreateFunction",
        json!({"FunctionName": name, "Runtime": "python3.12", "Role": role, "Handler": "index.handler",
            "Code": {"ZipFile": base64::encode(&zip)}, "Timeout": 10}),
    )
    .await
    .map(|r| r.json)
}

#[tokio::test]
async fn a_function_runs_as_its_role_and_iam_resolves_the_session() {
    if !have("python3") {
        return;
    }
    let w = World::with_mode(Mode::Enforce);
    let role = create_role(&w, "fn-role", TRUST).await;
    w.peer(
        "iam",
        "PutRolePolicy",
        json!({"RoleName": "fn-role", "PolicyName": "send", "PolicyDocument": SEND_ONLY}),
    )
    .await;
    function_with_role(&w, "f", &role, WHOAMI).await.unwrap();
    let got = w.invoke("f", &json!({})).await;
    let (key, token) = (
        got["Payload"]["key"].as_str().unwrap(),
        got["Payload"]["token"].as_str().unwrap(),
    );
    assert!(
        key.starts_with("ASIA"),
        "the function holds a session, not the test key: {key}"
    );
    let as_function = aws_wire::service::Caller {
        access_key_id: key.to_owned(),
        session_token: Some(token.to_owned()),
        ..World::caller()
    };
    let queue = format!("arn:aws:sqs:us-east-1:{ACCOUNT}:q");
    let allowed = w
        .peers
        .authorize(&as_function, vec![Need::new("sqs:SendMessage", &queue)])
        .await;
    assert!(allowed.is_ok(), "{allowed:?}");
    let denied = w
        .peers
        .authorize(&as_function, vec![Need::new("sqs:ReceiveMessage", &queue)])
        .await
        .unwrap_err();
    assert!(
        denied
            .message
            .contains("is not authorized to perform: sqs:ReceiveMessage"),
        "{}",
        denied.message
    );
    w.stop().await;
}

#[tokio::test]
async fn a_role_lambda_cannot_assume_is_refused_when_iam_enforces() {
    let w = World::with_mode(Mode::Enforce);
    let ec2 = create_role(&w, "ec2-role", &TRUST.replace("lambda", "ec2")).await;
    let e = function_with_role(&w, "f", &ec2, WORKS).await.unwrap_err();
    assert_eq!(
        e.message,
        "The role defined for the function cannot be assumed by Lambda."
    );
    let absent = function_with_role(
        &w,
        "f",
        &format!("arn:aws:iam::{ACCOUNT}:role/nobody"),
        WORKS,
    )
    .await
    .unwrap_err();
    assert_eq!(absent.code, "InvalidParameterValueException");
    // Not enforcing, a role the account has never heard of is accepted: a template names roles a local account may not have.
    let soft = World::with_mode(Mode::Soft);
    function_with_role(
        &soft,
        "f",
        &format!("arn:aws:iam::{ACCOUNT}:role/nobody"),
        WORKS,
    )
    .await
    .unwrap();
    soft.stop().await;
    w.stop().await;
}

#[tokio::test]
async fn an_event_invocation_is_retried_and_its_result_goes_to_the_destination() {
    if !have("python3") {
        return;
    }
    let w = World::new();
    let role = format!("arn:aws:iam::{ACCOUNT}:role/r");
    function_with_role(&w, "bad", &role, FAILS).await.unwrap();
    function_with_role(&w, "good", &role, WORKS).await.unwrap();
    let url = w
        .peer("sqs", "CreateQueue", json!({"QueueName": "results"}))
        .await["QueueUrl"]
        .as_str()
        .unwrap()
        .to_owned();
    let queue_arn = format!("arn:aws:sqs:us-east-1:{ACCOUNT}:results");
    let destinations =
        json!({"OnFailure": {"Destination": queue_arn}, "OnSuccess": {"Destination": queue_arn}});
    for name in ["bad", "good"] {
        w.ok("PutFunctionEventInvokeConfig", json!({"FunctionName": name, "MaximumRetryAttempts": 0, "DestinationConfig": destinations})).await;
        let accepted = w.ok("Invoke", json!({"FunctionName": name, "InvocationType": "Event", "Payload": base64::encode(br#"{"n":1}"#)})).await;
        assert_eq!(accepted["StatusCode"], 202);
    }
    let conditions = std::sync::Mutex::new(Vec::new());
    w.eventually("both results", || async {
        let got = w
            .peer(
                "sqs",
                "ReceiveMessage",
                json!({"QueueUrl": url, "MaxNumberOfMessages": 10}),
            )
            .await;
        for m in got["Messages"].as_array().into_iter().flatten() {
            let doc: Value = serde_json::from_str(m["Body"].as_str().unwrap()).unwrap();
            conditions.lock().unwrap().push(doc["requestContext"]["condition"].as_str().unwrap().to_owned());
            assert_eq!(doc["requestPayload"], json!({"n": 1}));
        }
        conditions.lock().unwrap().len() >= 2
    })
    .await;
    let mut conditions = conditions.into_inner().unwrap();
    conditions.sort();
    assert_eq!(conditions, ["RetriesExhausted", "Success"]);
    w.stop().await;
}

#[tokio::test]
async fn reserved_concurrency_of_zero_throttles_and_a_dry_run_runs_nothing() {
    let w = World::new();
    let role = format!("arn:aws:iam::{ACCOUNT}:role/r");
    function_with_role(&w, "f", &role, WORKS).await.unwrap();
    w.ok(
        "PutFunctionConcurrency",
        json!({"FunctionName": "f", "ReservedConcurrentExecutions": 0}),
    )
    .await;
    let e = w
        .err(
            "Invoke",
            json!({"FunctionName": "f", "Payload": base64::encode(b"{}")}),
        )
        .await;
    assert_eq!(
        (e.status, e.code.as_ref()),
        (429, "TooManyRequestsException")
    );
    let dry = w
        .ok(
            "Invoke",
            json!({"FunctionName": "f", "InvocationType": "DryRun"}),
        )
        .await;
    assert_eq!(dry["StatusCode"], 204);
    let bad = w.err("Invoke", json!({"FunctionName": "f", "InvocationType": "DryRun", "Payload": base64::encode(b"{not json")})).await;
    assert_eq!(bad.code, "InvalidRequestContentException");
    w.stop().await;
}

#[tokio::test]
async fn a_container_image_function_is_kept_and_its_invoke_says_why_it_cannot_run() {
    let w = World::new();
    let made = w
        .ok("CreateFunction", json!({"FunctionName": "img", "PackageType": "Image", "Role": format!("arn:aws:iam::{ACCOUNT}:role/r"),
            "Code": {"ImageUri": "123.dkr.ecr.us-east-1.amazonaws.com/x:1"}}))
        .await;
    assert_eq!(made["PackageType"], "Image");
    let got = w.ok("GetFunction", json!({"FunctionName": "img"})).await;
    assert_eq!(
        got["Code"]["ImageUri"],
        "123.dkr.ecr.us-east-1.amazonaws.com/x:1"
    );
    let e = w.err("Invoke", json!({"FunctionName": "img"})).await;
    assert!(
        e.message
            .starts_with("Container-image functions cannot run on doze-aws"),
        "{}",
        e.message
    );
    w.stop().await;
}

#[tokio::test]
async fn a_streamed_response_reaches_the_caller_chunk_by_chunk() {
    if !have("python3") {
        return;
    }
    let w = World::new();
    let role = format!("arn:aws:iam::{ACCOUNT}:role/r");
    function_with_role(&w, "f", &role, WORKS).await.unwrap();
    let reply: RestReply = w
        .call(
            "InvokeWithResponseStream",
            json!({"FunctionName": "f", "Payload": base64::encode(br#"{"a":1}"#)}),
        )
        .await
        .unwrap();
    assert_eq!(reply.json["StatusCode"], 200);
    let mut body = reply.body.unwrap();
    let mut bytes = Vec::new();
    while let Some(chunk) = std::future::poll_fn(|cx| body.as_mut().poll_next(cx)).await {
        bytes.extend_from_slice(&chunk.unwrap());
    }
    let text = String::from_utf8_lossy(&bytes);
    assert!(
        text.contains("PayloadChunk")
            && text.contains(r#"{"ok": {"a": 1}}"#)
            && text.contains("InvokeComplete"),
        "{text}"
    );
    w.stop().await;
}

#[tokio::test]
async fn a_function_url_turns_a_request_into_an_event_and_the_answer_into_a_response() {
    if !have("python3") {
        return;
    }
    let w = World::new();
    let role = format!("arn:aws:iam::{ACCOUNT}:role/r");
    let source = b"import json\ndef handler(event, context):\n    return {'statusCode': 201, 'headers': {'X-Seen': event['rawPath']}, 'body': json.dumps({'q': event.get('queryStringParameters'), 'body': event.get('body'), 'm': event['requestContext']['http']['method']})}\n";
    function_with_role(&w, "u", &role, source).await.unwrap();
    let url = w
        .ok(
            "CreateFunctionUrlConfig",
            json!({"FunctionName": "u", "AuthType": "NONE"}),
        )
        .await;
    let host = url["FunctionUrl"]
        .as_str()
        .unwrap()
        .trim_start_matches("http://")
        .trim_end_matches('/')
        .to_owned();
    let request = |host: &str, path: &str| WebRequest {
        method: "POST".into(),
        host: host.into(),
        path: path.into(),
        query: "a=1&b=x+y".into(),
        headers: vec![("content-type".into(), "text/plain".into())],
        body: bytes::Bytes::from_static(b"hello"),
        endpoint: "http://127.0.0.1:4566".into(),
    };
    let web = w.lambda.web().unwrap();
    assert!(web.owns(&host, "/orders/7"));
    let reply = web.serve(request(&host, "/orders/7")).await;
    assert_eq!(reply.status, 201);
    assert!(
        reply
            .headers
            .contains(&("x-seen".to_owned(), "/orders/7".to_owned()))
    );
    let body: Value = serde_json::from_slice(&reply.body).unwrap();
    assert_eq!(
        body,
        json!({"q": {"a": "1", "b": "x y"}, "body": "hello", "m": "POST"})
    );
    let id = host.split('.').next().unwrap();
    let by_path = web
        .serve(request(
            "127.0.0.1:4566",
            &format!("/_aws/lambda-url/us-east-1/{id}/p"),
        ))
        .await;
    assert_eq!(by_path.status, 201);
    assert_eq!(
        web.serve(request("nope.lambda-url.us-east-1.localhost:4566", "/"))
            .await
            .status,
        404
    );
    w.stop().await;
}

#[tokio::test]
async fn code_run_in_place_is_picked_up_when_the_directory_changes() {
    if !have("python3") {
        return;
    }
    let w = World::new();
    let dir = std::env::temp_dir().join(format!("doze-lambda-local-{}", std::process::id()));
    drop(std::fs::remove_dir_all(&dir));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("index.py"), "def handler(e, c):\n    return 1\n").unwrap();
    w.ok(
        "CreateFunction",
        json!({"FunctionName": "l", "Runtime": "python3.12", "Role": format!("arn:aws:iam::{ACCOUNT}:role/r"), "Handler": "index.handler",
            "Code": {"S3Bucket": "_local_", "S3Key": dir.to_string_lossy()}}),
    )
    .await;
    assert_eq!(w.invoke("l", &json!({})).await["Payload"], 1);
    let v1 = w.ok("PublishVersion", json!({"FunctionName": "l"})).await["Version"]
        .as_str()
        .unwrap()
        .to_owned();
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    std::fs::write(
        dir.join("index.py"),
        "def handler(e, c):\n    return 2\n  \n",
    )
    .unwrap();
    assert_eq!(w.invoke("l", &json!({})).await["Payload"], 2);
    // The published version is a copy: the directory changed under `$LATEST` and not under it.
    let frozen = w
        .ok(
            "Invoke",
            json!({"FunctionName": "l", "Qualifier": v1, "Payload": base64::encode(b"{}")}),
        )
        .await;
    assert_eq!(
        base64::decode(frozen["Payload"].as_str().unwrap()).unwrap(),
        b"1"
    );
    drop(std::fs::remove_dir_all(&dir));
    w.stop().await;
}
