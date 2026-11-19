//! The operations of stage apigw-ops: documentation parts and versions (and their export), client certificates, VPC links and model
//! templates. Usage, imported keys and the test invocations are in `invoke.rs`.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use serde_json::{Value, json};

use super::World;

#[tokio::test]
async fn documentation_parts_and_versions_are_kept_checked_and_exported() {
    let w = World::new();
    let (api, rid) = w.rest_api_with("pets").await;
    w.mock(&api, &rid, "{\"statusCode\": 200}", "{}").await;
    let part = w
        .rest("CreateDocumentationPart", json!({"restApiId": api, "location": {"type": "METHOD", "path": "/pets", "method": "GET"},
            "properties": "{\"summary\": \"List pets\", \"description\": \"Every pet\"}"}))
        .await
        .unwrap();
    assert_eq!(
        part["location"],
        json!({"type": "METHOD", "path": "/pets", "method": "GET"})
    );
    let id = part["id"].as_str().unwrap().to_owned();
    let again = w
        .rest("CreateDocumentationPart", json!({"restApiId": api, "location": {"type": "METHOD", "path": "/pets", "method": "GET"}, "properties": "{}"}))
        .await
        .unwrap_err();
    assert_eq!(again.code, "ConflictException");
    let bad = w
        .rest("CreateDocumentationPart", json!({"restApiId": api, "location": {"type": "API", "path": "/x"}, "properties": "{}"}))
        .await
        .unwrap_err();
    assert_eq!(bad.code, "BadRequestException");
    let not_json = w
        .rest(
            "CreateDocumentationPart",
            json!({"restApiId": api, "location": {"type": "API"}, "properties": "[1]"}),
        )
        .await
        .unwrap_err();
    assert_eq!(not_json.code, "BadRequestException");
    w.rest("CreateDocumentationPart", json!({"restApiId": api, "location": {"type": "API"}, "properties": "{\"description\": \"Pets API\"}"}))
        .await
        .unwrap();
    let methods = w
        .rest(
            "GetDocumentationParts",
            json!({"restApiId": api, "type": "METHOD"}),
        )
        .await
        .unwrap();
    assert_eq!(methods["item"].as_array().unwrap().len(), 1);
    let up = w
        .rest("UpdateDocumentationPart", json!({"restApiId": api, "documentationPartId": id,
            "patchOperations": [{"op": "replace", "path": "/properties", "value": "{\"summary\": \"All pets\"}"}]}))
        .await
        .unwrap();
    assert_eq!(up["properties"], "{\"summary\": \"All pets\"}");

    w.rest(
        "CreateDeployment",
        json!({"restApiId": api, "stageName": "dev"}),
    )
    .await
    .unwrap();
    let v = w
        .rest("CreateDocumentationVersion", json!({"restApiId": api, "documentationVersion": "1.0", "stageName": "dev", "description": "first"}))
        .await
        .unwrap();
    assert_eq!(v["version"], "1.0");
    assert!(v.get("_parts").is_none());
    let stage = w
        .rest("GetStage", json!({"restApiId": api, "stageName": "dev"}))
        .await
        .unwrap();
    assert_eq!(stage["documentationVersion"], "1.0");
    let export = w
        .rest("GetExport", json!({"restApiId": api, "stageName": "dev", "exportType": "oas30", "parameters": {"extensions": "documentation"}}))
        .await
        .unwrap();
    let text = aws_core::base64::decode(export["body"].as_str().unwrap()).unwrap();
    let doc: Value = serde_json::from_slice(&text).unwrap();
    assert_eq!(doc["info"]["description"], "Pets API");
    assert_eq!(doc["paths"]["/pets"]["get"]["summary"], "All pets");
    assert_eq!(doc["x-amazon-apigateway-documentation"]["version"], "1.0");
    assert_eq!(
        doc["x-amazon-apigateway-documentation"]["documentationParts"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    // Import into a new API: the parts come back.
    let other = w.rest("CreateRestApi", json!({"name": "o"})).await.unwrap();
    let oid = other["id"].as_str().unwrap();
    let imported = w
        .rest(
            "ImportDocumentationParts",
            json!({"restApiId": oid, "mode": "overwrite", "body": aws_core::base64::encode(&text)}),
        )
        .await
        .unwrap();
    assert_eq!(imported["ids"].as_array().unwrap().len(), 2);
    let versions = w
        .rest("GetDocumentationVersions", json!({"restApiId": api}))
        .await
        .unwrap();
    assert_eq!(versions["item"][0]["description"], "first");
    let published = w
        .rest(
            "DeleteDocumentationVersion",
            json!({"restApiId": api, "documentationVersion": "1.0"}),
        )
        .await
        .unwrap_err();
    assert_eq!(
        published.message,
        "Cannot delete documentation version because there are API Stages associated with it."
    );
    w.rest("DeleteStage", json!({"restApiId": api, "stageName": "dev"}))
        .await
        .unwrap();
    w.rest(
        "DeleteDocumentationVersion",
        json!({"restApiId": api, "documentationVersion": "1.0"}),
    )
    .await
    .unwrap();
    w.rest(
        "DeleteDocumentationPart",
        json!({"restApiId": api, "documentationPartId": id}),
    )
    .await
    .unwrap();
    let gone = w
        .rest(
            "GetDocumentationPart",
            json!({"restApiId": api, "documentationPartId": id}),
        )
        .await
        .unwrap_err();
    assert_eq!(gone.code, "NotFoundException");
}

#[tokio::test]
async fn a_client_certificate_is_a_real_certificate_for_a_year() {
    let w = World::new();
    let c = w
        .rest(
            "GenerateClientCertificate",
            json!({"description": "backend", "tags": {"a": "1"}}),
        )
        .await
        .unwrap();
    let pem = c["pemEncodedCertificate"].as_str().unwrap();
    assert!(
        pem.starts_with("-----BEGIN CERTIFICATE-----\n")
            && pem.ends_with("-----END CERTIFICATE-----\n")
    );
    let made = c["createdDate"].as_f64().unwrap();
    assert!((c["expirationDate"].as_f64().unwrap() - made - 365.0 * 86_400.0).abs() < 1.0);
    assert!(c.get("_key").is_none());
    let id = c["clientCertificateId"].as_str().unwrap();
    let got = w
        .rest("GetClientCertificate", json!({"clientCertificateId": id}))
        .await
        .unwrap();
    assert_eq!(got["pemEncodedCertificate"], c["pemEncodedCertificate"]);
    assert_eq!(got["tags"], json!({"a": "1"}));
    let up = w
        .rest("UpdateClientCertificate", json!({"clientCertificateId": id, "patchOperations": [{"op": "replace", "path": "/description", "value": "new"}]}))
        .await
        .unwrap();
    assert_eq!(up["description"], "new");
    let all = w.rest("GetClientCertificates", json!({})).await.unwrap();
    assert_eq!(all["item"].as_array().unwrap().len(), 1);
    w.rest(
        "DeleteClientCertificate",
        json!({"clientCertificateId": id}),
    )
    .await
    .unwrap();
    let gone = w
        .rest("GetClientCertificate", json!({"clientCertificateId": id}))
        .await
        .unwrap_err();
    assert_eq!(gone.code, "NotFoundException");
}

#[tokio::test]
async fn vpc_links_go_from_pending_to_available_and_integrations_name_them() {
    let w = World::new();
    let nlb = "arn:aws:elasticloadbalancing:us-east-1:000000000000:loadbalancer/net/n/1";
    let link = w
        .rest("CreateVpcLink", json!({"name": "l", "targetArns": [nlb]}))
        .await
        .unwrap();
    assert_eq!(link["status"], "PENDING");
    let id = link["id"].as_str().unwrap();
    let read = w
        .rest("GetVpcLink", json!({"vpcLinkId": id}))
        .await
        .unwrap();
    assert_eq!(read["status"], "AVAILABLE");
    let wrong = w
        .rest("CreateVpcLink", json!({"name": "l", "targetArns": ["x"]}))
        .await
        .unwrap_err();
    assert_eq!(wrong.code, "BadRequestException");
    let (api, rid) = w.rest_api_with("p").await;
    let put = |conn: &str| {
        json!({"restApiId": api, "resourceId": rid, "requestHttpMethod": "GET", "type": "HTTP_PROXY", "httpMethod": "GET",
            "uri": "http://127.0.0.1:9/", "connectionType": "VPC_LINK", "connectionId": conn})
    };
    let missing = w.rest("PutIntegration", put("nope")).await.unwrap_err();
    assert_eq!(missing.code, "NotFoundException");
    let ok = w.rest("PutIntegration", put(id)).await.unwrap();
    assert_eq!(ok["connectionType"], "VPC_LINK");
    w.rest("UpdateVpcLink", json!({"vpcLinkId": id, "patchOperations": [{"op": "replace", "path": "/name", "value": "m"}]}))
        .await
        .unwrap();
    assert_eq!(
        w.rest("GetVpcLinks", json!({})).await.unwrap()["item"][0]["name"],
        "m"
    );
    w.rest("DeleteVpcLink", json!({"vpcLinkId": id}))
        .await
        .unwrap();

    let v2 = w
        .http(
            "CreateVpcLink",
            json!({"name": "v", "subnetIds": ["subnet-1"]}),
        )
        .await
        .unwrap();
    assert_eq!(v2["vpcLinkStatus"], "PENDING");
    assert_eq!(v2["vpcLinkVersion"], "V2");
    let vid = v2["vpcLinkId"].as_str().unwrap();
    assert_eq!(
        w.http("GetVpcLink", json!({"VpcLinkId": vid}))
            .await
            .unwrap()["vpcLinkStatus"],
        "AVAILABLE"
    );
    assert_eq!(
        w.http("GetVpcLinks", json!({})).await.unwrap()["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    w.http("DeleteVpcLink", json!({"VpcLinkId": vid}))
        .await
        .unwrap();
    assert!(
        w.http("GetVpcLink", json!({"VpcLinkId": vid}))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn a_model_template_follows_the_schema() {
    let w = World::new();
    let api = w.rest("CreateRestApi", json!({"name": "m"})).await.unwrap();
    let id = api["id"].as_str().unwrap();
    let empty = w
        .rest(
            "GetModelTemplate",
            json!({"restApiId": id, "modelName": "Empty"}),
        )
        .await
        .unwrap();
    assert_eq!(empty["value"], "#set($inputRoot = $input.path('$'))\n{ }");
    w.rest("CreateModel", json!({"restApiId": id, "name": "Pet", "contentType": "application/json",
        "schema": "{\"type\": \"object\", \"properties\": {\"name\": {\"type\": \"string\"}, \"age\": {\"type\": \"integer\"}}}"}))
        .await
        .unwrap();
    let pet = w
        .rest(
            "GetModelTemplate",
            json!({"restApiId": id, "modelName": "Pet"}),
        )
        .await
        .unwrap();
    let t = pet["value"].as_str().unwrap();
    assert!(
        t.contains("\"name\" : \"$inputRoot.name\"") && t.contains("\"age\" : $inputRoot.age"),
        "{t}"
    );
    let none = w
        .rest(
            "GetModelTemplate",
            json!({"restApiId": id, "modelName": "Nope"}),
        )
        .await
        .unwrap_err();
    assert_eq!(none.code, "NotFoundException");

    let h = w.http("CreateApi", json!({"name": "w", "protocolType": "WEBSOCKET", "routeSelectionExpression": "$request.body.action"})).await.unwrap();
    let hid = h["apiId"].as_str().unwrap();
    let m = w.http("CreateModel", json!({"ApiId": hid, "name": "M", "schema": "{\"type\": \"object\", \"properties\": {\"ok\": {\"type\": \"boolean\"}}}"})).await.unwrap();
    let t2 = w
        .http(
            "GetModelTemplate",
            json!({"ApiId": hid, "ModelId": m["modelId"]}),
        )
        .await
        .unwrap();
    assert!(
        t2["value"]
            .as_str()
            .unwrap()
            .contains("\"ok\" : $inputRoot.ok")
    );
}
