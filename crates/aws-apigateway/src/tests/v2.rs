//! HTTP APIs: the control plane, routing and CORS on the plane, a JWT authorizer, and quick create.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use serde_json::{Value, json};

use super::{World, body, header};

#[tokio::test]
async fn an_http_api_routes_answers_cors_and_checks_tokens() {
    let w = World::new();
    let url = super::plane_backend(200).await;
    let api = w
        .http("CreateApi", json!({"name": "h", "protocolType": "HTTP",
            "corsConfiguration": {"allowOrigins": ["http://app.test"], "allowMethods": ["GET"], "maxAge": 60}}))
        .await
        .unwrap();
    let id = api["apiId"].as_str().unwrap().to_owned();
    assert_eq!(
        api["routeSelectionExpression"],
        "$request.method $request.path"
    );
    let integ = w
        .http("CreateIntegration", json!({"ApiId": id, "integrationType": "HTTP_PROXY", "integrationMethod": "GET", "integrationUri": format!("{url}/svc/{{id}}"), "payloadFormatVersion": "1.0"}))
        .await
        .unwrap();
    assert_eq!(integ["payloadFormatVersion"], "1.0");
    let target = format!("integrations/{}", integ["integrationId"].as_str().unwrap());
    let bad = w
        .http("CreateRoute", json!({"ApiId": id, "routeKey": "orders"}))
        .await
        .unwrap_err();
    assert_eq!(bad.code, "BadRequestException");
    w.http(
        "CreateRoute",
        json!({"ApiId": id, "routeKey": "GET /orders/{id}", "target": target}),
    )
    .await
    .unwrap();
    let idp = super::tls::Issuer::start(aws_webhook::test_server::Ca::get().identity()).await;
    let issuer = idp.url.as_str();
    let auth = w
        .http("CreateAuthorizer", json!({"ApiId": id, "name": "jwt", "authorizerType": "JWT", "identitySource": ["$request.header.Authorization"],
            "jwtConfiguration": {"issuer": issuer, "audience": ["me"]}}))
        .await
        .unwrap();
    w.http(
        "CreateRoute",
        json!({"ApiId": id, "routeKey": "GET /secret", "target": target, "authorizationType": "JWT",
        "authorizerId": auth["authorizerId"], "authorizationScopes": ["read"]}),
    )
    .await
    .unwrap();
    w.http(
        "CreateStage",
        json!({"ApiId": id, "stageName": "$default", "autoDeploy": true}),
    )
    .await
    .unwrap();

    let r = w
        .web(
            "GET",
            &format!("/_aws/execute-api/{id}/orders/42"),
            "",
            &[("origin", "http://app.test")],
        )
        .await;
    assert_eq!(r.status, 200);
    assert_eq!(body(&r)["line"], "GET /svc/42 HTTP/1.1");
    assert_eq!(
        header(&r, "Access-Control-Allow-Origin"),
        Some("http://app.test")
    );
    let pre = w
        .web(
            "OPTIONS",
            &format!("/_aws/execute-api/{id}/orders/42"),
            "",
            &[
                ("origin", "http://app.test"),
                ("access-control-request-method", "GET"),
            ],
        )
        .await;
    assert_eq!(
        (pre.status, header(&pre, "Access-Control-Max-Age")),
        (204, Some("60"))
    );
    let nothing = w
        .web("GET", &format!("/_aws/execute-api/{id}/elsewhere"), "", &[])
        .await;
    assert_eq!(
        (nothing.status, body(&nothing)),
        (404, json!({"message": "Not Found"}))
    );

    let no_token = w
        .web("GET", &format!("/_aws/execute-api/{id}/secret"), "", &[])
        .await;
    assert_eq!(no_token.status, 401);
    let token = |claims: Value| super::tls::Issuer::sign(&claims);
    let far = 4_000_000_000_i64;
    let wrong = token(json!({"iss": issuer, "aud": "other", "exp": far, "scope": "read"}));
    assert_eq!(
        w.web(
            "GET",
            &format!("/_aws/execute-api/{id}/secret"),
            "",
            &[("authorization", &wrong)]
        )
        .await
        .status,
        401
    );
    let narrow = token(json!({"iss": issuer, "aud": "me", "exp": far, "scope": "write"}));
    assert_eq!(
        w.web(
            "GET",
            &format!("/_aws/execute-api/{id}/secret"),
            "",
            &[("authorization", &narrow)]
        )
        .await
        .status,
        403
    );
    let good = token(json!({"iss": issuer, "aud": "me", "exp": far, "scope": "read"}));
    assert_eq!(
        w.web(
            "GET",
            &format!("/_aws/execute-api/{id}/secret"),
            "",
            &[("authorization", &good)]
        )
        .await
        .status,
        200
    );

    let routes = w.http("GetRoutes", json!({"ApiId": id})).await.unwrap();
    assert_eq!(routes["items"].as_array().unwrap().len(), 2);
    let in_use = w
        .http(
            "DeleteIntegration",
            json!({"ApiId": id, "IntegrationId": integ["integrationId"]}),
        )
        .await
        .unwrap_err();
    assert_eq!(in_use.code, "ConflictException");
    let refused = w.http("ListPortals", json!({})).await.unwrap_err();
    assert_eq!(refused.status, 501);
}

#[tokio::test]
async fn quick_create_stages_deployments_and_settings() {
    let w = World::new();
    let api = w
        .http("CreateApi", json!({"name": "q", "protocolType": "HTTP", "target": "arn:aws:lambda:us-east-1:000000000000:function:f"}))
        .await
        .unwrap();
    let id = api["apiId"].as_str().unwrap().to_owned();
    let routes = w.http("GetRoutes", json!({"ApiId": id})).await.unwrap();
    assert_eq!(routes["items"][0]["routeKey"], "$default");
    let stage = w
        .http("GetStage", json!({"ApiId": id, "StageName": "$default"}))
        .await
        .unwrap();
    assert_eq!(stage["autoDeploy"], true);
    w.http(
        "CreateStage",
        json!({"ApiId": id, "stageName": "manual", "stageVariables": {"a": "1"}}),
    )
    .await
    .unwrap();
    let dep = w
        .http(
            "CreateDeployment",
            json!({"ApiId": id, "stageName": "manual"}),
        )
        .await
        .unwrap();
    assert_eq!(dep["deploymentStatus"], "DEPLOYED");
    let s = w
        .http(
            "UpdateStage",
            json!({"ApiId": id, "StageName": "manual", "stageVariables": {"b": "2"},
        "routeSettings": {"GET /x": {"throttlingRateLimit": 5.0}}}),
        )
        .await
        .unwrap();
    assert_eq!(s["stageVariables"], json!({"a": "1", "b": "2"}));
    assert_eq!(s["deploymentId"], dep["deploymentId"]);
    w.http(
        "DeleteRouteSettings",
        json!({"ApiId": id, "StageName": "manual", "RouteKey": "GET /x"}),
    )
    .await
    .unwrap();
    let busy = w
        .http(
            "DeleteDeployment",
            json!({"ApiId": id, "DeploymentId": dep["deploymentId"]}),
        )
        .await
        .unwrap_err();
    assert_eq!(busy.code, "BadRequestException");
    let arn = format!("arn:aws:apigateway:us-east-1::/apis/{id}");
    w.http(
        "TagResource",
        json!({"ResourceArn": arn, "tags": {"a": "b"}}),
    )
    .await
    .unwrap();
    assert_eq!(
        w.http("GetTags", json!({"ResourceArn": arn}))
            .await
            .unwrap()["tags"],
        json!({"a": "b"})
    );
    let absent = w
        .http("GetApi", json!({"ApiId": "nope"}))
        .await
        .unwrap_err();
    assert_eq!(
        absent.message,
        "Invalid API identifier specified 000000000000:nope"
    );
}

/// Model `Stage.AutoDeploy`: "updates to an API automatically trigger a new deployment".
#[tokio::test]
async fn an_auto_deploying_stage_names_a_deployment_and_follows_the_api() {
    let w = World::new();
    let api = w
        .http("CreateApi", json!({"name": "a", "protocolType": "HTTP"}))
        .await
        .unwrap();
    let id = api["apiId"].as_str().unwrap().to_owned();
    let made = w
        .http(
            "CreateStage",
            json!({"ApiId": id, "stageName": "live", "autoDeploy": true}),
        )
        .await
        .unwrap();
    let first = made["deploymentId"].as_str().unwrap().to_owned();
    let deployment = w
        .http("GetDeployment", json!({"ApiId": id, "DeploymentId": first}))
        .await
        .unwrap();
    assert_eq!(
        (
            deployment["deploymentStatus"].as_str(),
            deployment["autoDeployed"].as_bool()
        ),
        (Some("DEPLOYED"), Some(true))
    );
    // A new route deploys again; the deployment nothing names any more is gone.
    w.http("CreateRoute", json!({"ApiId": id, "routeKey": "GET /x"}))
        .await
        .unwrap();
    let stage = w
        .http("GetStage", json!({"ApiId": id, "StageName": "live"}))
        .await
        .unwrap();
    let second = stage["deploymentId"].as_str().unwrap().to_owned();
    assert_ne!(second, first);
    assert!(stage["lastDeploymentStatusMessage"].as_str().is_some());
    let all = w
        .http("GetDeployments", json!({"ApiId": id}))
        .await
        .unwrap();
    assert_eq!(all["items"].as_array().unwrap().len(), 1);
    // A stage without autoDeploy has no deployment until one is made for it.
    let manual = w
        .http("CreateStage", json!({"ApiId": id, "stageName": "manual"}))
        .await
        .unwrap();
    assert!(manual.get("deploymentId").is_none());
}
