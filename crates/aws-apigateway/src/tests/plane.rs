//! The execute-api plane of a REST API: MOCK and mapping templates, deployments as snapshots, gateway responses, API keys, request
//! validation, and HTTP integrations against a real local server.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::{World, body, header};

#[tokio::test]
async fn a_mock_answers_from_its_templates_and_a_change_waits_for_a_deployment() {
    let w = World::new();
    let (api, rid) = w.rest_api_with("items").await;
    w.mock(
        &api,
        &rid,
        "#if($input.params('fail') == 'yes'){\"statusCode\": 500}#else{\"statusCode\": 200}#end",
        "{\"stage\": \"$context.stage\", \"q\": \"$input.params('q')\", \"n\": 1}",
    )
    .await;
    w.rest(
        "CreateDeployment",
        json!({"restApiId": api, "stageName": "dev"}),
    )
    .await
    .unwrap();
    let r = w
        .web(
            "GET",
            &format!("/_aws/execute-api/{api}/dev/items?q=hello"),
            "",
            &[],
        )
        .await;
    assert_eq!(r.status, 200);
    assert_eq!(body(&r), json!({"stage": "dev", "q": "hello", "n": 1}));
    // A 500 from the template matches no selection pattern: the default response answers.
    let failed = w
        .web(
            "GET",
            &format!("/_aws/execute-api/{api}/dev/items?fail=yes"),
            "",
            &[],
        )
        .await;
    assert_eq!(
        failed.status, 200,
        "a MOCK status with no pattern for it takes the default response"
    );
    // A change is not served until it is deployed.
    w.rest("UpdateIntegrationResponse", json!({"restApiId": api, "resourceId": rid, "httpMethod": "GET", "statusCode": "200",
        "patchOperations": [{"op": "replace", "path": "/responseTemplates/application~1json", "value": "{\"n\": 2}"}]}))
        .await
        .unwrap();
    let still = w
        .web(
            "GET",
            &format!("/_aws/execute-api/{api}/dev/items"),
            "",
            &[],
        )
        .await;
    assert_eq!(body(&still)["n"], 1);
    w.rest(
        "CreateDeployment",
        json!({"restApiId": api, "stageName": "dev"}),
    )
    .await
    .unwrap();
    let now = w
        .web(
            "GET",
            &format!("/_aws/execute-api/{api}/dev/items"),
            "",
            &[],
        )
        .await;
    assert_eq!(body(&now), json!({"n": 2}));
    // The recorded answers for what matches nothing.
    let none = w
        .web(
            "GET",
            &format!("/_aws/execute-api/{api}/dev/nowhere"),
            "",
            &[],
        )
        .await;
    assert_eq!(
        (none.status, body(&none)),
        (403, json!({"message": "Missing Authentication Token"}))
    );
    let post = w
        .web(
            "POST",
            &format!("/_aws/execute-api/{api}/dev/items"),
            "",
            &[],
        )
        .await;
    assert_eq!(post.status, 403);
    let no_stage = w
        .web(
            "GET",
            &format!("/_aws/execute-api/{api}/nostage/items"),
            "",
            &[],
        )
        .await;
    assert_eq!(no_stage.status, 403);
}

#[tokio::test]
async fn gateway_responses_keys_and_validation_answer_as_configured() {
    let w = World::new();
    let (api, rid) = w.rest_api_with("k").await;
    w.mock(&api, &rid, "{\"statusCode\": 200}", "{\"ok\": true}")
        .await;
    w.rest("PutGatewayResponse", json!({"restApiId": api, "responseType": "MISSING_AUTHENTICATION_TOKEN", "statusCode": "404",
        "responseParameters": {"gatewayresponse.header.X-Why": "'lost'"},
        "responseTemplates": {"application/json": "{\"error\": $context.error.messageString}"}})).await.unwrap();
    let v = w
        .rest(
            "CreateRequestValidator",
            json!({"restApiId": api, "name": "params", "validateRequestParameters": true}),
        )
        .await
        .unwrap();
    w.rest("UpdateMethod", json!({"restApiId": api, "resourceId": rid, "httpMethod": "GET", "patchOperations": [
        {"op": "replace", "path": "/apiKeyRequired", "value": "true"},
        {"op": "replace", "path": "/requestValidatorId", "value": v["id"]},
        {"op": "add", "path": "/requestParameters/method.request.querystring.must", "value": "true"}]})).await.unwrap();
    w.rest(
        "CreateDeployment",
        json!({"restApiId": api, "stageName": "s"}),
    )
    .await
    .unwrap();
    let lost = w
        .web(
            "GET",
            &format!("/_aws/execute-api/{api}/s/elsewhere"),
            "",
            &[],
        )
        .await;
    assert_eq!(
        (lost.status, body(&lost), header(&lost, "X-Why")),
        (
            404,
            json!({"error": "Missing Authentication Token"}),
            Some("lost")
        )
    );
    let keyless = w
        .web(
            "GET",
            &format!("/_aws/execute-api/{api}/s/k?must=1"),
            "",
            &[],
        )
        .await;
    assert_eq!(
        (keyless.status, body(&keyless)),
        (403, json!({"message": "Forbidden"}))
    );
    let key = w
        .rest(
            "CreateApiKey",
            json!({"name": "k", "enabled": true, "value": "abcdefghijklmnopqrstuvwxyz"}),
        )
        .await
        .unwrap();
    let plan = w
        .rest(
            "CreateUsagePlan",
            json!({"name": "p", "apiStages": [{"apiId": api, "stage": "s"}]}),
        )
        .await
        .unwrap();
    w.rest(
        "CreateUsagePlanKey",
        json!({"usagePlanId": plan["id"], "keyId": key["id"], "keyType": "API_KEY"}),
    )
    .await
    .unwrap();
    let with_key = [("x-api-key", "abcdefghijklmnopqrstuvwxyz")];
    let missing = w
        .web(
            "GET",
            &format!("/_aws/execute-api/{api}/s/k"),
            "",
            &with_key,
        )
        .await;
    assert_eq!(
        (missing.status, body(&missing)),
        (
            400,
            json!({"message": "Missing required request parameters: [must]"})
        )
    );
    let ok = w
        .web(
            "GET",
            &format!("/_aws/execute-api/{api}/s/k?must=1"),
            "",
            &with_key,
        )
        .await;
    assert_eq!((ok.status, body(&ok)), (200, json!({"ok": true})));
}

/// A one-shot HTTP server: answers every request with `status` and echoes the request line and body into the answer's body.
pub(crate) async fn backend(status: u16) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut s, _)) = listener.accept().await {
            let mut buf = vec![0u8; 8192];
            let n = s.read(&mut buf).await.unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]).into_owned();
            let line = req.lines().next().unwrap_or_default().to_owned();
            let body_in = req.split("\r\n\r\n").nth(1).unwrap_or_default().to_owned();
            let out = json!({"line": line, "body": body_in}).to_string();
            let answer = format!(
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nX-Back: yes\r\nContent-Length: {}\r\n\r\n{out}",
                out.len()
            );
            drop(s.write_all(answer.as_bytes()).await);
        }
    });
    format!("http://{addr}")
}

#[tokio::test]
async fn http_proxy_and_http_integrations_reach_a_real_server() {
    let w = World::new();
    let url = backend(201).await;
    let (api, rid) = w.rest_api_with("{proxy+}").await;
    let mut put = json!({"restApiId": api, "resourceId": rid, "requestHttpMethod": "GET", "type": "HTTP_PROXY",
        "httpMethod": "ANY", "uri": format!("{url}/up/{{proxy}}")});
    w.rest("PutIntegration", put.take()).await.unwrap();
    w.rest(
        "CreateDeployment",
        json!({"restApiId": api, "stageName": "s"}),
    )
    .await
    .unwrap();
    let r = w
        .web(
            "GET",
            &format!("/_aws/execute-api/{api}/s/a/b?x=1"),
            "",
            &[],
        )
        .await;
    assert_eq!(r.status, 201);
    assert_eq!(body(&r)["line"], "GET /up/a/b?x=1 HTTP/1.1");
    assert_eq!(header(&r, "X-Back"), Some("yes"));

    let (api2, rid2) = w.rest_api_with("send").await;
    let at = json!({"restApiId": api2, "resourceId": rid2, "httpMethod": "GET"});
    w.rest("PutIntegration", json!({"restApiId": api2, "resourceId": rid2, "requestHttpMethod": "GET", "type": "HTTP", "httpMethod": "POST",
        "uri": format!("{url}/hook"), "requestTemplates": {"application/json": "{\"said\": \"$input.params('w')\"}"}})).await.unwrap();
    let mut mr = at.clone();
    mr["statusCode"] = json!("200");
    w.rest("PutMethodResponse", mr.clone()).await.unwrap();
    mr["selectionPattern"] = json!("2\\d\\d");
    mr["responseTemplates"] = json!({"application/json": "{\"back\": $input.json('$.body')}"});
    w.rest("PutIntegrationResponse", mr).await.unwrap();
    w.rest(
        "CreateDeployment",
        json!({"restApiId": api2, "stageName": "s"}),
    )
    .await
    .unwrap();
    let r = w
        .web(
            "GET",
            &format!("/_aws/execute-api/{api2}/s/send?w=hi"),
            "",
            &[],
        )
        .await;
    assert_eq!(r.status, 200);
    assert_eq!(body(&r), json!({"back": "{\"said\": \"hi\"}"}));
}
