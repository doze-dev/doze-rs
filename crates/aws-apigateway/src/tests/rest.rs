//! The REST API control plane: the tree, methods, the `Update*` operations with patches, deployments, stages, keys and plans, tags,
//! domain names, and the refusals by name.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use serde_json::json;

use aws_wire::service::Service;

use super::World;

#[tokio::test]
async fn a_rest_api_is_built_patched_and_deployed() {
    let w = World::new();
    let (api, rid) = w.rest_api_with("ping").await;
    w.mock(&api, &rid, "{\"statusCode\": 200}", "{\"pong\": true}")
        .await;
    let at = json!({"restApiId": api, "resourceId": rid, "httpMethod": "GET"});
    // UpdateMethod, UpdateIntegration, UpdateMethodResponse and UpdateIntegrationResponse take patches (Go answered 405).
    let m = w
        .rest("UpdateMethod", json!({"restApiId": api, "resourceId": rid, "httpMethod": "GET",
            "patchOperations": [{"op": "replace", "path": "/apiKeyRequired", "value": "true"},
                                {"op": "add", "path": "/requestParameters/method.request.querystring.q", "value": "false"}]}))
        .await
        .unwrap();
    assert_eq!(m["apiKeyRequired"], true);
    assert_eq!(
        m["requestParameters"]["method.request.querystring.q"],
        false
    );
    let i = w
        .rest(
            "UpdateIntegration",
            json!({"restApiId": api, "resourceId": rid, "httpMethod": "GET",
            "patchOperations": [{"op": "replace", "path": "/timeoutInMillis", "value": "1000"}]}),
        )
        .await
        .unwrap();
    assert_eq!(i["timeoutInMillis"], 1000);
    let ir = w
        .rest("UpdateIntegrationResponse", json!({"restApiId": api, "resourceId": rid, "httpMethod": "GET", "statusCode": "200",
            "patchOperations": [{"op": "add", "path": "/responseParameters/method.response.header.X-A", "value": "'1'"}]}))
        .await
        .unwrap();
    assert_eq!(
        ir["responseParameters"]["method.response.header.X-A"],
        "'1'"
    );
    let bad = w
        .rest(
            "UpdateMethod",
            json!({"restApiId": api, "resourceId": rid, "httpMethod": "GET",
            "patchOperations": [{"op": "replace", "path": "/nonsense", "value": "1"}]}),
        )
        .await
        .unwrap_err();
    assert_eq!(bad.code, "BadRequestException");
    let got = w.rest("GetMethod", at.clone()).await.unwrap();
    assert_eq!(
        got["methodIntegration"]["integrationResponses"]["200"]["statusCode"],
        "200"
    );

    let dep = w
        .rest(
            "CreateDeployment",
            json!({"restApiId": api, "stageName": "dev", "variables": {"a": "1"}}),
        )
        .await
        .unwrap();
    let stage = w
        .rest(
            "UpdateStage",
            json!({"restApiId": api, "stageName": "dev", "patchOperations": [
            {"op": "replace", "path": "/*/*/logging/loglevel", "value": "INFO"},
            {"op": "replace", "path": "/~1ping/GET/throttling/rateLimit", "value": "5"},
            {"op": "replace", "path": "/cacheClusterEnabled", "value": "true"},
            {"op": "replace", "path": "/accessLogSettings/format", "value": "$context.requestId"},
            {"op": "add", "path": "/variables/b", "value": "2"}]}),
        )
        .await
        .unwrap();
    assert_eq!(stage["methodSettings"]["*/*"]["loggingLevel"], "INFO");
    assert_eq!(stage["methodSettings"]["*/*"]["throttlingBurstLimit"], 5000);
    assert_eq!(
        stage["methodSettings"]["ping/GET"]["throttlingRateLimit"],
        5.0
    );
    assert_eq!(
        (
            stage["cacheClusterEnabled"].clone(),
            stage["cacheClusterStatus"].clone()
        ),
        (json!(true), json!("AVAILABLE"))
    );
    assert_eq!(stage["variables"], json!({"a": "1", "b": "2"}));
    let summary = w
        .rest(
            "GetDeployment",
            json!({"restApiId": api, "deploymentId": dep["id"], "embed": ["apisummary"]}),
        )
        .await
        .unwrap();
    assert_eq!(
        summary["apiSummary"]["/ping"]["GET"]["apiKeyRequired"],
        true
    );
    let gone = w
        .rest(
            "DeleteDeployment",
            json!({"restApiId": api, "deploymentId": dep["id"]}),
        )
        .await
        .unwrap_err();
    assert_eq!(
        gone.message,
        "Active stages pointing to this deployment must be moved or deleted"
    );
    let twice = w
        .rest("PutMethod", json!({"restApiId": api, "resourceId": rid, "httpMethod": "GET", "authorizationType": "NONE"}))
        .await
        .unwrap_err();
    assert_eq!(twice.code, "ConflictException");
}

#[tokio::test]
async fn resources_move_and_delete_with_their_subtree() {
    let w = World::new();
    let (api, a) = w.rest_api_with("a").await;
    let b = w
        .rest(
            "CreateResource",
            json!({"restApiId": api, "parentId": a, "pathPart": "{id}"}),
        )
        .await
        .unwrap();
    let c = w
        .rest(
            "CreateResource",
            json!({"restApiId": api, "parentId": b["id"], "pathPart": "c"}),
        )
        .await
        .unwrap();
    assert_eq!(c["path"], "/a/{id}/c");
    let second = w
        .rest(
            "CreateResource",
            json!({"restApiId": api, "parentId": a, "pathPart": "{other}"}),
        )
        .await
        .unwrap_err();
    assert_eq!(second.code, "BadRequestException");
    let moved = w
        .rest("UpdateResource", json!({"restApiId": api, "resourceId": a, "patchOperations": [{"op": "replace", "path": "/pathPart", "value": "z"}]}))
        .await
        .unwrap();
    assert_eq!(moved["path"], "/z");
    let c_now = w
        .rest(
            "GetResource",
            json!({"restApiId": api, "resourceId": c["id"]}),
        )
        .await
        .unwrap();
    assert_eq!(c_now["path"], "/z/{id}/c");
    let list = w
        .rest(
            "GetResources",
            json!({"restApiId": api, "embed": ["methods"]}),
        )
        .await
        .unwrap();
    assert_eq!(list["item"].as_array().unwrap().len(), 4);
    assert!(
        list["item"][3]["resourceMethods"]["GET"]["httpMethod"].is_string()
            || list["item"][1]["resourceMethods"]["GET"]["httpMethod"].is_string()
    );
    w.rest("DeleteResource", json!({"restApiId": api, "resourceId": a}))
        .await
        .unwrap();
    let left = w
        .rest("GetResources", json!({"restApiId": api}))
        .await
        .unwrap();
    assert_eq!(left["item"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn keys_plans_account_tags_domains_and_parts() {
    let w = World::new();
    let (api, rid) = w.rest_api_with("p").await;
    w.mock(&api, &rid, "{\"statusCode\": 200}", "{}").await;
    w.rest(
        "CreateDeployment",
        json!({"restApiId": api, "stageName": "prod"}),
    )
    .await
    .unwrap();
    let key = w
        .rest("CreateApiKey", json!({"name": "k", "enabled": true}))
        .await
        .unwrap();
    assert_eq!(key["value"].as_str().unwrap().len(), 40);
    let hidden = w
        .rest("GetApiKey", json!({"apiKey": key["id"]}))
        .await
        .unwrap();
    assert!(hidden.get("value").is_none());
    let plan = w.rest("CreateUsagePlan", json!({"name": "p", "apiStages": [{"apiId": api, "stage": "prod"}], "throttle": {"rateLimit": 1.5}})).await.unwrap();
    let nope = w
        .rest(
            "CreateUsagePlan",
            json!({"name": "q", "apiStages": [{"apiId": api, "stage": "nope"}]}),
        )
        .await
        .unwrap_err();
    assert_eq!(nope.code, "BadRequestException");
    let pk = w
        .rest(
            "CreateUsagePlanKey",
            json!({"usagePlanId": plan["id"], "keyId": key["id"], "keyType": "API_KEY"}),
        )
        .await
        .unwrap();
    assert_eq!(pk["type"], "API_KEY");
    let keys = w
        .rest("GetUsagePlanKeys", json!({"usagePlanId": plan["id"]}))
        .await
        .unwrap();
    assert_eq!(keys["item"].as_array().unwrap().len(), 1);
    let patched = w
        .rest("UpdateUsagePlan", json!({"usagePlanId": plan["id"], "patchOperations": [
            {"op": "replace", "path": "/quota/limit", "value": "100"}, {"op": "replace", "path": "/quota/period", "value": "DAY"},
            {"op": "remove", "path": "/apiStages", "value": format!("{api}:prod")}]}))
        .await
        .unwrap();
    assert_eq!(patched["quota"], json!({"limit": 100, "period": "DAY"}));
    assert_eq!(patched["apiStages"], json!([]));
    w.rest("DeleteApiKey", json!({"apiKey": key["id"]}))
        .await
        .unwrap();
    let account = w.rest("UpdateAccount", json!({"patchOperations": [{"op": "replace", "path": "/cloudwatchRoleArn", "value": "arn:aws:iam::1:role/r"}]})).await.unwrap();
    assert_eq!(account["cloudwatchRoleArn"], "arn:aws:iam::1:role/r");
    let arn = format!("arn:aws:apigateway:us-east-1::/restapis/{api}");
    w.rest(
        "TagResource",
        json!({"resourceArn": arn, "tags": {"team": "a"}}),
    )
    .await
    .unwrap();
    assert_eq!(
        w.rest("GetTags", json!({"resourceArn": arn}))
            .await
            .unwrap()["tags"],
        json!({"team": "a"})
    );
    let d = w.rest("CreateDomainName", json!({"domainName": "api.example.test", "regionalCertificateArn": "arn:aws:acm:x", "endpointConfiguration": {"types": ["REGIONAL"]}})).await.unwrap();
    assert_eq!(d["domainNameStatus"], "AVAILABLE");
    w.rest(
        "CreateBasePathMapping",
        json!({"domainName": "api.example.test", "restApiId": api, "stage": "prod"}),
    )
    .await
    .unwrap();
    let maps = w
        .rest(
            "GetBasePathMappings",
            json!({"domainName": "api.example.test"}),
        )
        .await
        .unwrap();
    assert_eq!(maps["item"][0]["basePath"], "(none)");
    let v = w
        .rest(
            "CreateRequestValidator",
            json!({"restApiId": api, "name": "all", "validateRequestBody": true}),
        )
        .await
        .unwrap();
    assert_eq!(v["validateRequestParameters"], false);
    let models = w
        .rest("GetModels", json!({"restApiId": api}))
        .await
        .unwrap();
    assert_eq!(
        models["item"].as_array().unwrap().len(),
        2,
        "Empty and Error"
    );
    let responses = w
        .rest(
            "GetGatewayResponses",
            json!({"restApiId": api, "limit": 500}),
        )
        .await
        .unwrap();
    assert_eq!(responses["item"].as_array().unwrap().len(), 21);
    let custom = w.rest("PutGatewayResponse", json!({"restApiId": api, "responseType": "MISSING_AUTHENTICATION_TOKEN", "statusCode": "404"})).await.unwrap();
    assert_eq!(custom["defaultResponse"], false);
    let refused = w
        .rest("GenerateClientCertificate", json!({}))
        .await
        .unwrap_err();
    assert_eq!(refused.status, 501);
    assert!(!w.v1.serves(w.h1_op("GetSdk")));
}

impl World {
    fn h1_op(&self, name: &str) -> u32 {
        aws_model_api_gateway::tables::TABLES.find_op(name).unwrap()
    }
}
