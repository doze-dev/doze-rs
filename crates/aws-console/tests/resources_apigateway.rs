//! API Gateway's pages: the kind operations they make (a REST API with its resources, a method's four calls, a stage and a
//! deployment; an HTTP API's four creates, its CORS and its authorizer; a usage plan with its keys), and the usage plans kind.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_lines,
    clippy::cognitive_complexity
)]

mod support;
#[path = "resources/world.rs"]
mod world;

use aws_peers::Mode;
use serde_json::{Value, json};
use world::{field, world};

const BASE: &str = "/api/v1/resources/apigateway";

fn op(kind: &str, operation: &str) -> String {
    format!("{BASE}/{kind}/op/{operation}")
}

async fn make(w: &world::World, kind: &str, operation: &str, input: Value) -> Value {
    let (status, out) = w.send("POST", &op(kind, operation), &input).await;
    assert_eq!(status, 200, "{operation}: {out}");
    out
}

#[tokio::test]
async fn a_rest_api_is_built_resource_by_resource_and_method_by_method_through_the_kinds_operations()
 {
    let w = world("apigw-rest", Mode::Soft, &[]);
    let api = make(
        &w,
        "restapis",
        "CreateRestApi",
        json!({"name": "larder-orders", "description": "Order intake",
            "endpointConfiguration": {"types": ["REGIONAL"], "ipAddressType": "ipv4"}}),
    )
    .await;
    let id = api["id"].as_str().unwrap().to_owned();
    let root = api["rootResourceId"].as_str().unwrap().to_owned();
    assert_eq!(api["endpointConfiguration"]["types"][0], "REGIONAL");
    let products = make(
        &w,
        "restapis",
        "CreateResource",
        json!({"restApiId": id, "parentId": root, "pathPart": "products"}),
    )
    .await;
    assert_eq!(products["path"], "/products");
    let rid = products["id"].as_str().unwrap().to_owned();
    // A method is four calls: the method, its integration, and a response of each.
    make(
        &w,
        "restapis",
        "PutMethod",
        json!({"restApiId": id, "resourceId": rid, "httpMethod": "GET", "authorizationType": "NONE"}),
    )
    .await;
    make(
        &w,
        "restapis",
        "PutIntegration",
        json!({"restApiId": id, "resourceId": rid, "requestHttpMethod": "GET", "type": "MOCK",
            "requestTemplates": {"application/json": "{\"statusCode\": 200}"}}),
    )
    .await;
    make(
        &w,
        "restapis",
        "PutMethodResponse",
        json!({"restApiId": id, "resourceId": rid, "httpMethod": "GET", "statusCode": "200"}),
    )
    .await;
    make(
        &w,
        "restapis",
        "PutIntegrationResponse",
        json!({"restApiId": id, "resourceId": rid, "httpMethod": "GET", "statusCode": "200"}),
    )
    .await;
    // The page reads the whole tree in one call, methods under their resources.
    let tree = make(
        &w,
        "restapis",
        "GetResources",
        json!({"restApiId": id, "embed": ["methods"], "limit": 500}),
    )
    .await;
    let items = tree["item"].as_array().unwrap();
    assert_eq!(items.len(), 2, "{tree}");
    let get = &items.iter().find(|i| i["path"] == "/products").unwrap()["resourceMethods"]["GET"];
    assert_eq!(get["methodIntegration"]["type"], "MOCK");
    assert_eq!(get["methodResponses"]["200"]["statusCode"], "200");
    // A change of the method goes in as a patch, as the page's settings do.
    let patched = make(
        &w,
        "restapis",
        "UpdateMethod",
        json!({"restApiId": id, "resourceId": rid, "httpMethod": "GET",
            "patchOperations": [{"op": "replace", "path": "/apiKeyRequired", "value": "true"}]}),
    )
    .await;
    assert_eq!(patched["apiKeyRequired"], true);
    // Deploy: a stage made by the deployment, its variables and description changed by a patch.
    let dep = make(
        &w,
        "restapis",
        "CreateDeployment",
        json!({"restApiId": id, "stageName": "prod", "description": "First deploy",
            "variables": {"catalogHost": "catalog.larder.example.com"}}),
    )
    .await;
    let stage = make(
        &w,
        "restapis",
        "UpdateStage",
        json!({"restApiId": id, "stageName": "prod",
            "patchOperations": [{"op": "replace", "path": "/description", "value": "Production"}]}),
    )
    .await;
    assert_eq!(stage["deploymentId"], dep["id"]);
    assert_eq!(stage["description"], "Production");
    let history = make(&w, "restapis", "GetDeployments", json!({"restApiId": id})).await;
    assert_eq!(history["item"].as_array().unwrap().len(), 1);
    // A deployment a stage serves is refused; a second one, made by hand, is deleted; so are the method, the resource and the stage.
    let (status, refused) = w
        .send(
            "POST",
            &op("restapis", "DeleteDeployment"),
            &json!({"restApiId": id, "deploymentId": dep["id"]}),
        )
        .await;
    assert_eq!(status, 400, "{refused}");
    assert!(
        refused["message"].as_str().unwrap().contains("Active stages"),
        "{refused}"
    );
    let spare = make(&w, "restapis", "CreateDeployment", json!({"restApiId": id})).await;
    make(
        &w,
        "restapis",
        "DeleteDeployment",
        json!({"restApiId": id, "deploymentId": spare["id"]}),
    )
    .await;
    make(
        &w,
        "restapis",
        "DeleteMethod",
        json!({"restApiId": id, "resourceId": rid, "httpMethod": "GET"}),
    )
    .await;
    make(
        &w,
        "restapis",
        "DeleteResource",
        json!({"restApiId": id, "resourceId": rid}),
    )
    .await;
    make(
        &w,
        "restapis",
        "DeleteStage",
        json!({"restApiId": id, "stageName": "prod"}),
    )
    .await;
    make(
        &w,
        "restapis",
        "DeleteDeployment",
        json!({"restApiId": id, "deploymentId": dep["id"]}),
    )
    .await;
    let after = make(&w, "restapis", "GetDeployments", json!({"restApiId": id})).await;
    assert!(after["item"].as_array().is_none_or(Vec::is_empty), "{after}");
    // The calls are the console's, and show in Live as its own.
    assert!(
        w.live().iter().any(|l| l == "console apigateway PutMethod"),
        "{:?}",
        w.live()
    );
    // The API's page reads what the generic detail says of it.
    let (_, detail) = w.get(&format!("{BASE}/restapis/{id}")).await;
    assert_eq!(field(&detail, "Name")["value"], "larder-orders");
    // Deleting the API is the delete endpoint's: an operation the kind does not list is not made, whatever it names.
    let (status, refused) = w
        .send(
            "POST",
            &op("restapis", "DeleteRestApi"),
            &json!({"restApiId": id}),
        )
        .await;
    assert_eq!(
        (status, refused["code"].as_str()),
        (404, Some("NOT_IN_PAGE"))
    );
}

#[tokio::test]
async fn an_http_api_is_made_by_the_wizards_four_calls_and_given_cors_and_an_authorizer() {
    let w = world("apigw-http", Mode::Soft, &[]);
    let api = make(
        &w,
        "apis",
        "CreateApi",
        json!({"name": "larder-reports", "protocolType": "HTTP"}),
    )
    .await;
    let id = api["apiId"].as_str().unwrap().to_owned();
    let integration = make(
        &w,
        "apis",
        "CreateIntegration",
        json!({"ApiId": id, "integrationType": "AWS_PROXY", "payloadFormatVersion": "2.0",
            "integrationUri": "arn:aws:lambda:us-east-1:000000000000:function:export-report"}),
    )
    .await;
    let iid = integration["integrationId"].as_str().unwrap().to_owned();
    let route = make(
        &w,
        "apis",
        "CreateRoute",
        json!({"ApiId": id, "routeKey": "ANY /export-report", "target": format!("integrations/{iid}")}),
    )
    .await;
    make(
        &w,
        "apis",
        "CreateStage",
        json!({"ApiId": id, "stageName": "$default", "autoDeploy": true}),
    )
    .await;
    // The automatic stage names a deployment, and the history has it.
    let stages = make(&w, "apis", "GetStages", json!({"ApiId": id})).await;
    let deployment = stages["items"][0]["deploymentId"].as_str().unwrap();
    let history = make(&w, "apis", "GetDeployments", json!({"ApiId": id})).await;
    assert_eq!(history["items"][0]["deploymentId"], deployment);
    assert_eq!(history["items"][0]["autoDeployed"], true);
    // The stage serves that deployment, so its delete is refused (the page says so in words and does not offer it).
    let (status, refused) = w
        .send(
            "POST",
            &op("apis", "DeleteDeployment"),
            &json!({"ApiId": id, "DeploymentId": deployment}),
        )
        .await;
    assert_eq!(status, 400, "{refused}");
    // Once the stage is gone the deployment can go.
    make(
        &w,
        "apis",
        "DeleteStage",
        json!({"ApiId": id, "StageName": "$default"}),
    )
    .await;
    make(
        &w,
        "apis",
        "DeleteDeployment",
        json!({"ApiId": id, "DeploymentId": deployment}),
    )
    .await;
    make(
        &w,
        "apis",
        "CreateStage",
        json!({"ApiId": id, "stageName": "$default", "autoDeploy": true}),
    )
    .await;
    // CORS is a part of the API itself.
    let cors = json!({"allowOrigins": ["https://admin.larder.example.com"], "allowMethods": ["GET", "POST"], "maxAge": 300});
    let updated = make(
        &w,
        "apis",
        "UpdateApi",
        json!({"ApiId": id, "corsConfiguration": cors}),
    )
    .await;
    assert_eq!(updated["corsConfiguration"]["maxAge"], 300);
    // An authorizer, then the route is attached to it.
    let auth = make(
        &w,
        "apis",
        "CreateAuthorizer",
        json!({"ApiId": id, "name": "admin-request-auth", "authorizerType": "REQUEST",
            "authorizerUri": "arn:aws:apigateway:us-east-1:lambda:path/2015-03-31/functions/arn:aws:lambda:us-east-1:000000000000:function:admin-auth/invocations",
            "authorizerPayloadFormatVersion": "2.0", "enableSimpleResponses": true,
            "identitySource": ["$request.header.Authorization"], "authorizerResultTtlInSeconds": 300}),
    )
    .await;
    let attached = make(
        &w,
        "apis",
        "UpdateRoute",
        json!({"ApiId": id, "RouteId": route["routeId"], "authorizationType": "CUSTOM",
            "authorizerId": auth["authorizerId"]}),
    )
    .await;
    assert_eq!(attached["authorizationType"], "CUSTOM");
    let routes = make(&w, "apis", "GetRoutes", json!({"ApiId": id})).await;
    assert_eq!(routes["items"][0]["routeKey"], "ANY /export-report");
    let (_, detail) = w.get(&format!("{BASE}/apis/{id}")).await;
    assert_eq!(field(&detail, "Protocol")["value"], "HTTP");
}

#[tokio::test]
async fn a_usage_plan_takes_a_key_made_in_the_same_step_and_is_a_kind_of_its_own() {
    let w = world("apigw-plans", Mode::Soft, &[]);
    let plan = make(
        &w,
        "usageplans",
        "CreateUsagePlan",
        json!({"name": "partners-premium", "description": "Premium tier",
            "throttle": {"rateLimit": 200, "burstLimit": 400}, "quota": {"limit": 100_000, "period": "MONTH"}}),
    )
    .await;
    let id = plan["id"].as_str().unwrap().to_owned();
    assert_eq!(plan["throttle"]["burstLimit"], 400);
    let key = make(
        &w,
        "usageplans",
        "CreateApiKey",
        json!({"name": "storefront-berlin", "enabled": true}),
    )
    .await;
    make(
        &w,
        "usageplans",
        "CreateUsagePlanKey",
        json!({"usagePlanId": id, "keyId": key["id"], "keyType": "API_KEY"}),
    )
    .await;
    let keys = make(
        &w,
        "usageplans",
        "GetUsagePlanKeys",
        json!({"usagePlanId": id}),
    )
    .await;
    assert_eq!(keys["item"][0]["name"], "storefront-berlin");
    let all = make(
        &w,
        "usageplans",
        "GetApiKeys",
        json!({"includeValues": false}),
    )
    .await;
    assert_eq!(all["item"].as_array().unwrap().len(), 1);
    // The list and the page are the generic ones' over GetUsagePlans and GetUsagePlan.
    let (status, list) = w.get(&format!("{BASE}/usageplans")).await;
    assert_eq!(status, 200, "{list}");
    assert_eq!(list["rows"][0]["label"], "partners-premium");
    assert_eq!(list["rows"][0]["name"], id);
    let (_, detail) = w.get(&format!("{BASE}/usageplans/{id}")).await;
    assert_eq!(field(&detail, "Quota period")["value"], "MONTH");
    assert_eq!(detail["subs"][0]["rows"].as_array().unwrap().len(), 1);
}
