//! OpenAPI import and export, both APIs.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use serde_json::json;

use super::{World, body};

const PETS: &str = r##"openapi: 3.0.1
info:
  title: pets
  version: "1"
x-amazon-apigateway-request-validators:
  all:
    validateRequestBody: true
    validateRequestParameters: true
paths:
  /pets/{id}:
    get:
      operationId: getPet
      parameters:
      - name: id
        in: path
        required: true
      x-amazon-apigateway-request-validator: all
      responses:
        "200":
          description: ok
          content:
            application/json:
              schema:
                $ref: "#/components/schemas/Pet"
      x-amazon-apigateway-integration:
        type: mock
        requestTemplates:
          application/json: '{"statusCode": 200}'
        responses:
          default:
            statusCode: "200"
            responseTemplates:
              application/json: '{"id": "$input.params(''id'')"}'
components:
  schemas:
    Pet:
      type: object
      properties:
        id:
          type: string
"##;

#[tokio::test]
async fn a_rest_api_is_imported_served_exported_and_overwritten() {
    let w = World::new();
    let made = w
        .rest(
            "ImportRestApi",
            json!({"body": aws_core::base64::encode(PETS.as_bytes())}),
        )
        .await
        .unwrap();
    assert_eq!(made["name"], "pets");
    let api = made["id"].as_str().unwrap().to_owned();
    let resources = w
        .rest(
            "GetResources",
            json!({"restApiId": api, "embed": ["methods"]}),
        )
        .await
        .unwrap();
    let paths: Vec<&str> = resources["item"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["path"].as_str().unwrap())
        .collect();
    assert_eq!(paths, ["/", "/pets", "/pets/{id}"]);
    let models = w
        .rest("GetModel", json!({"restApiId": api, "modelName": "Pet"}))
        .await
        .unwrap();
    assert!(models["schema"].as_str().unwrap().contains("object"));
    w.rest(
        "CreateDeployment",
        json!({"restApiId": api, "stageName": "v1"}),
    )
    .await
    .unwrap();
    let r = w
        .web(
            "GET",
            &format!("/_aws/execute-api/{api}/v1/pets/7"),
            "",
            &[],
        )
        .await;
    assert_eq!((r.status, body(&r)), (200, json!({"id": "7"})));

    let exported = w
        .rest("GetExport", json!({"restApiId": api, "stageName": "v1", "exportType": "oas30", "parameters": {"extensions": "integrations"}}))
        .await
        .unwrap();
    let text =
        String::from_utf8(aws_core::base64::decode(exported["body"].as_str().unwrap()).unwrap())
            .unwrap();
    let doc: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        doc["paths"]["/pets/{id}"]["get"]["x-amazon-apigateway-integration"]["type"],
        "mock"
    );
    assert_eq!(doc["components"]["schemas"]["Pet"]["type"], "object");

    // The export imports back; `overwrite` replaces the tree.
    let again = w
        .rest("ImportRestApi", json!({"body": exported["body"]}))
        .await
        .unwrap();
    assert_eq!(again["name"], "pets");
    let small = r#"{"openapi": "3.0.1", "info": {"title": "small"}, "paths": {"/only": {"get": {"x-amazon-apigateway-integration": {"type": "mock"}}}}}"#;
    w.rest("PutRestApi", json!({"restApiId": api, "mode": "overwrite", "body": aws_core::base64::encode(small.as_bytes())})).await.unwrap();
    let after = w
        .rest("GetResources", json!({"restApiId": api}))
        .await
        .unwrap();
    let paths: Vec<&str> = after["item"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["path"].as_str().unwrap())
        .collect();
    assert_eq!(paths, ["/", "/only"]);
    let broken = w
        .rest(
            "ImportRestApi",
            json!({"body": aws_core::base64::encode(b"not: [an openapi")}),
        )
        .await
        .unwrap_err();
    assert_eq!(broken.code, "BadRequestException");
}

#[tokio::test]
async fn an_http_api_is_imported_reimported_and_exported() {
    let w = World::new();
    let doc = json!({"openapi": "3.0.1", "info": {"title": "h", "version": "1"},
    "x-amazon-apigateway-cors": {"allowOrigins": ["*"]},
    "components": {"securitySchemes": {"jwt": {"type": "oauth2", "x-amazon-apigateway-authorizer": {"type": "jwt",
        "jwtConfiguration": {"issuer": "https://i.test", "audience": ["a"]}}}}},
    "paths": {
        "/a": {"get": {"security": [{"jwt": ["read"]}], "x-amazon-apigateway-integration": {"type": "http_proxy", "httpMethod": "GET", "uri": "http://127.0.0.1:9/a", "payloadFormatVersion": "1.0"}}},
        "$default": {"x-amazon-apigateway-any-method": {"x-amazon-apigateway-integration": {"type": "aws_proxy", "uri": "arn:aws:lambda:us-east-1:0:function:f", "payloadFormatVersion": "2.0"}}}
    }});
    let api = w
        .http("ImportApi", json!({"body": doc.to_string()}))
        .await
        .unwrap();
    let id = api["apiId"].as_str().unwrap().to_owned();
    assert_eq!(api["corsConfiguration"]["allowOrigins"], json!(["*"]));
    let routes = w.http("GetRoutes", json!({"ApiId": id})).await.unwrap();
    let mut keys: Vec<&str> = routes["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["routeKey"].as_str().unwrap())
        .collect();
    keys.sort_unstable();
    assert_eq!(keys, ["$default", "GET /a"]);
    let secured = routes["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["routeKey"] == "GET /a")
        .unwrap();
    assert_eq!(
        (
            secured["authorizationType"].as_str(),
            secured["authorizationScopes"].clone()
        ),
        (Some("JWT"), json!(["read"]))
    );
    let exported = w
        .http(
            "ExportApi",
            json!({"ApiId": id, "Specification": "OAS30", "OutputType": "YAML"}),
        )
        .await
        .unwrap();
    let yaml =
        String::from_utf8(aws_core::base64::decode(exported["body"].as_str().unwrap()).unwrap())
            .unwrap();
    assert!(yaml.contains("\"/a\":"), "{yaml}");
    let re = json!({"openapi": "3.0.1", "info": {"title": "h2"}, "paths": {"/b": {"post": {"x-amazon-apigateway-integration": {"type": "http_proxy", "uri": "http://127.0.0.1:9/b"}}}}});
    w.http("ReimportApi", json!({"ApiId": id, "body": re.to_string()}))
        .await
        .unwrap();
    let routes = w.http("GetRoutes", json!({"ApiId": id})).await.unwrap();
    assert_eq!(routes["items"].as_array().unwrap().len(), 1);
    assert_eq!(routes["items"][0]["routeKey"], "POST /b");
}
