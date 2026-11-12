//! Management: the lifecycle of a function, versions, aliases, permissions, layers, URLs, tags and concurrency, with no process run.

use serde_json::json;

use super::World;

#[tokio::test]
async fn a_function_is_created_read_listed_changed_and_deleted() {
    let w = World::new();
    let made = w.create("f", "python3.12", "index.handler", &[("index.py", b"def handler(e, c): return e", 0o644)]).await;
    assert_eq!(made["FunctionName"], "f");
    assert_eq!(made["Version"], "$LATEST");
    assert_eq!(made["FunctionArn"], "arn:aws:lambda:us-east-1:111122223333:function:f");
    assert_eq!(made["MemorySize"], 128);
    assert_eq!(made["LoggingConfig"]["LogGroup"], "/aws/lambda/f");
    let got = w.ok("GetFunction", json!({"FunctionName": "f"})).await;
    assert_eq!(got["Configuration"]["CodeSha256"], made["CodeSha256"]);
    // The revision settles after creation, as AWS's does when the function becomes active.
    assert_ne!(got["Configuration"]["RevisionId"], made["RevisionId"]);
    let again = w.err("CreateFunction", json!({"FunctionName": "f", "Role": "arn:aws:iam::111122223333:role/r", "Runtime": "python3.12", "Handler": "a.b", "Code": {"ZipFile": aws_core::base64::encode(&crate::zipread::build(&[("a.py", b"x", 0o644)]))}})).await;
    assert_eq!((again.status, again.code.as_ref()), (409, "ResourceConflictException"));
    let list = w.ok("ListFunctions", json!({})).await;
    assert_eq!(list["Functions"].as_array().unwrap().len(), 1);
    let updated = w.ok("UpdateFunctionConfiguration", json!({"FunctionName": "f", "MemorySize": 256, "Environment": {"Variables": {"AA": "1"}}})).await;
    assert_eq!((updated["MemorySize"].as_i64(), updated["Environment"]["Variables"]["AA"].as_str()), (Some(256), Some("1")));
    let gone = w.ok("DeleteFunction", json!({"FunctionName": "f"})).await;
    assert_eq!(gone, json!({}));
    let missing = w.err("GetFunction", json!({"FunctionName": "f"})).await;
    assert_eq!(missing.message, "Function not found: arn:aws:lambda:us-east-1:111122223333:function:f");
    w.stop().await;
}

#[tokio::test]
async fn versions_freeze_and_aliases_point() {
    let w = World::new();
    w.create("f", "python3.12", "index.handler", &[("index.py", b"x", 0o644)]).await;
    let v1 = w.ok("PublishVersion", json!({"FunctionName": "f", "Description": "first"})).await;
    assert_eq!(v1["Version"], "1");
    // Nothing changed: the same version again.
    let same = w.ok("PublishVersion", json!({"FunctionName": "f"})).await;
    assert_eq!((same["Version"].clone(), same["RevisionId"].clone()), (v1["Version"].clone(), v1["RevisionId"].clone()));
    w.ok("UpdateFunctionConfiguration", json!({"FunctionName": "f", "Timeout": 20})).await;
    assert_eq!(w.ok("PublishVersion", json!({"FunctionName": "f"})).await["Version"], "2");
    let alias = w.ok("CreateAlias", json!({"FunctionName": "f", "Name": "live", "FunctionVersion": "1"})).await;
    assert_eq!(alias["AliasArn"], "arn:aws:lambda:us-east-1:111122223333:function:f:live");
    let by_alias = w.ok("GetFunctionConfiguration", json!({"FunctionName": "f", "Qualifier": "live"})).await;
    assert_eq!((by_alias["Version"].clone(), by_alias["Timeout"].clone()), (json!("1"), json!(10)));
    let versions: Vec<String> = w.ok("ListVersionsByFunction", json!({"FunctionName": "f"})).await["Versions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["Version"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(versions, ["$LATEST", "1", "2"]);
    let twice = w.err("CreateAlias", json!({"FunctionName": "f", "Name": "live", "FunctionVersion": "1"})).await;
    assert_eq!(twice.message, "Alias already exists: arn:aws:lambda:us-east-1:111122223333:function:f:live");
    let absent = w.err("CreateAlias", json!({"FunctionName": "f", "Name": "ghost", "FunctionVersion": "9"})).await;
    assert_eq!(absent.message, "Function not found: arn:aws:lambda:us-east-1:111122223333:function:f:9");
    let frozen = w.err("UpdateFunctionConfiguration", json!({"FunctionName": "f:1", "MemorySize": 512})).await;
    assert_eq!(frozen.message, "This API only supports unqualified ARN or $LATEST.");
    w.stop().await;
}

#[tokio::test]
async fn permissions_are_statements_in_one_policy() {
    let w = World::new();
    w.create("f", "python3.12", "index.handler", &[("index.py", b"x", 0o644)]).await;
    let none = w.err("GetPolicy", json!({"FunctionName": "f"})).await;
    assert_eq!((none.status, none.message.as_str()), (404, "The resource you requested does not exist."));
    let add = json!({"FunctionName": "f", "StatementId": "sns", "Action": "lambda:InvokeFunction", "Principal": "sns.amazonaws.com",
        "SourceArn": "arn:aws:sns:us-east-1:111122223333:t"});
    let added = w.ok("AddPermission", add.clone()).await;
    let statement: serde_json::Value = serde_json::from_str(added["Statement"].as_str().unwrap()).unwrap();
    assert_eq!(statement["Principal"], json!({"Service": "sns.amazonaws.com"}));
    assert_eq!(statement["Condition"], json!({"ArnLike": {"AWS:SourceArn": "arn:aws:sns:us-east-1:111122223333:t"}}));
    assert_eq!(w.err("AddPermission", add).await.status, 409);
    let policy = w.ok("GetPolicy", json!({"FunctionName": "f"})).await;
    let doc: serde_json::Value = serde_json::from_str(policy["Policy"].as_str().unwrap()).unwrap();
    assert_eq!(doc["Statement"].as_array().unwrap().len(), 1);
    w.ok("RemovePermission", json!({"FunctionName": "f", "StatementId": "sns"})).await;
    assert_eq!(w.err("RemovePermission", json!({"FunctionName": "f", "StatementId": "sns"})).await.message, "No policy is associated with the given resource.");
    w.stop().await;
}

#[tokio::test]
async fn layers_urls_tags_and_reserved_concurrency_are_kept() {
    let w = World::new();
    let layer_zip = crate::zipread::build(&[("python/helper.py", b"X = 1", 0o644)]);
    let layer = w.ok("PublishLayerVersion", json!({"LayerName": "lib", "Content": {"ZipFile": aws_core::base64::encode(&layer_zip)}, "CompatibleRuntimes": ["python3.12"]})).await;
    assert_eq!(layer["Version"], 1);
    assert_eq!(layer["LayerVersionArn"], "arn:aws:lambda:us-east-1:111122223333:layer:lib:1");
    let listed = w.ok("ListLayerVersions", json!({"LayerName": "lib"})).await;
    assert_eq!(listed["LayerVersions"].as_array().unwrap().len(), 1);
    let code = crate::zipread::build(&[("index.py", b"x", 0o644)]);
    let made = w
        .ok("CreateFunction", json!({"FunctionName": "f", "Runtime": "python3.12", "Role": "arn:aws:iam::111122223333:role/r", "Handler": "index.handler",
            "Code": {"ZipFile": aws_core::base64::encode(&code)}, "Layers": [layer["LayerVersionArn"]], "Tags": {"team": "a"}}))
        .await;
    assert_eq!(made["Layers"][0]["CodeSize"], layer["Content"]["CodeSize"]);
    let missing = w.err("UpdateFunctionConfiguration", json!({"FunctionName": "f", "Layers": ["arn:aws:lambda:us-east-1:111122223333:layer:lib:9"]})).await;
    assert_eq!(missing.code, "InvalidParameterValueException");
    let url = w.ok("CreateFunctionUrlConfig", json!({"FunctionName": "f", "AuthType": "NONE"})).await;
    assert!(url["FunctionUrl"].as_str().unwrap().contains(".lambda-url.us-east-1.localhost:4566/"), "{url}");
    assert_eq!(w.err("CreateFunctionUrlConfig", json!({"FunctionName": "f", "AuthType": "NONE"})).await.status, 409);
    let arn = made["FunctionArn"].as_str().unwrap().to_owned();
    w.ok("TagResource", json!({"Resource": arn, "Tags": {"env": "dev"}})).await;
    assert_eq!(w.ok("ListTags", json!({"Resource": arn})).await["Tags"], json!({"team": "a", "env": "dev"}));
    w.ok("PutFunctionConcurrency", json!({"FunctionName": "f", "ReservedConcurrentExecutions": 0})).await;
    assert_eq!(w.ok("GetFunctionConcurrency", json!({"FunctionName": "f"})).await["ReservedConcurrentExecutions"], 0);
    assert_eq!(w.ok("GetAccountSettings", json!({})).await["AccountUsage"]["FunctionCount"], 1);
    w.stop().await;
}

#[tokio::test]
async fn what_is_not_built_is_refused_by_name() {
    let w = World::new();
    for op in ["CreateCapacityProvider", "GetDurableExecution", "InvokeAsync", "PutFunctionScalingConfig"] {
        assert!(!aws_wire::service::Service::serves(&w.lambda, w.lambda_op(op)), "{op}");
    }
    assert!(aws_wire::service::Service::serves(&w.lambda, w.lambda_op("Invoke")));
    w.stop().await;
}
