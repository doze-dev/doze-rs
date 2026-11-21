//! Lambda's pages through the console: the list and the header of a function, the calls its tabs make (a version, an alias, a
//! permission, a URL, a trigger, the destinations), the layers, what a delete leaves, and the package the Code tab reads.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_lines
)]
#[path = "resources/code.rs"]
mod code;
mod support;
#[path = "resources/world.rs"]
mod world;

use aws_peers::Mode;
use serde_json::{Value, json};
use support::{get, send};
use world::{field, firsts, sub, world};

const FN: &str = "/api/v1/resources/lambda/functions";

async fn op(w: &world::World, name: &str, operation: &str, body: &Value) -> (u16, Value) {
    w.send("POST", &format!("{FN}/{name}/op/{operation}"), body)
        .await
}

#[tokio::test]
async fn a_function_says_what_it_is_and_what_can_be_done_to_it() {
    let w = world("lambda-page", Mode::Enforce, &[]);
    if !code::function(&w, "echo").await {
        return;
    }
    let (_, list) = w.get(FN).await;
    let names = |l: &Value| {
        l["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["name"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(names(&list), ["echo"]);
    let columns: Vec<&str> = list["columns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["title"].as_str().unwrap())
        .collect();
    assert_eq!(
        columns[..4],
        ["Description", "Package type", "Runtime", "Last modified"]
    );
    assert_eq!(list["sentence"], "1 function runs here.");
    let (_, d) = w.get(&format!("{FN}/echo")).await;
    let badges: Vec<&str> = d["badges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["text"].as_str().unwrap())
        .collect();
    assert_eq!(badges, ["Zip", "Active"]);
    assert_eq!(
        d["sentence"],
        "echo runs provided.al2023 with 128 MB and a 3 second timeout."
    );
    let verbs: Vec<&str> = d["verbs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["id"].as_str().unwrap())
        .collect();
    assert_eq!(verbs, ["trigger", "destination", "delete"]);
    assert_eq!(field(&d, "Ephemeral storage (MB)")["value"], "512");
    assert_eq!(field(&d, "Function URL")["value"], "None");
}

#[tokio::test]
async fn a_function_is_made_from_its_own_page_with_the_code_it_is_given() {
    let w = world("lambda-create", Mode::Enforce, &[]);
    if !code::function(&w, "first").await {
        return;
    }
    // The zip of the first function is the one the second is made from, read through the Code tab's own address.
    let (res, _) = send(&w.app, get(&format!("{FN}/first/code"))).await;
    assert_eq!(res.status(), 200);
    let app = w.app.clone();
    let res = tower::ServiceExt::oneshot(app, get(&format!("{FN}/first/code")))
        .await
        .unwrap();
    assert_eq!(
        res.headers().get("content-type").unwrap(),
        "application/zip"
    );
    let bytes = http_body_util::BodyExt::collect(res.into_body())
        .await
        .unwrap()
        .to_bytes();
    assert_eq!(&bytes[..2], b"PK");
    let (status, made) = w
        .send(
            "POST",
            &format!("{FN}/op/CreateFunction"),
            &json!({"FunctionName": "second", "Runtime": "provided.al2023", "Handler": "x",
                "Role": "arn:aws:iam::000000000000:role/fn-role",
                "Code": {"ZipFile": aws_core::base64::encode(&bytes)}}),
        )
        .await;
    assert_eq!(status, 200, "{made}");
    assert_eq!(made["FunctionName"], "second");
    // Other services' packages are not for reading, and neither is a function that is not there.
    let (res, _) = send(&w.app, get("/api/v1/resources/sqs/queues/q/code")).await;
    assert_eq!(res.status(), 404);
    let (res, _) = send(&w.app, get(&format!("{FN}/nope/code"))).await;
    assert_eq!(res.status(), 404);
}

#[tokio::test]
async fn versions_aliases_permissions_and_a_url_are_the_functions_own_calls() {
    let w = world("lambda-tabs", Mode::Enforce, &[]);
    if !code::function(&w, "echo").await {
        return;
    }
    let (s, v) = op(
        &w,
        "echo",
        "PublishVersion",
        &json!({"Description": "first deploy"}),
    )
    .await;
    assert_eq!((s, v["Version"].clone()), (200, json!("1")), "{v}");
    let (s, a) = op(
        &w,
        "echo",
        "CreateAlias",
        &json!({"Name": "live", "FunctionVersion": "1", "Description": "Production"}),
    )
    .await;
    assert_eq!(s, 200, "{a}");
    let (_, listed) = op(&w, "echo", "ListAliases", &json!({})).await;
    assert_eq!(listed["Aliases"][0]["Name"], "live");
    let (s, p) = op(
        &w,
        "echo",
        "AddPermission",
        &json!({"StatementId": "sns-alerts", "Action": "lambda:InvokeFunction", "Principal": "sns.amazonaws.com",
            "SourceArn": "arn:aws:sns:us-east-1:000000000000:alerts"}),
    )
    .await;
    assert_eq!(s, 200, "{p}");
    let (_, policy) = op(&w, "echo", "GetPolicy", &json!({})).await;
    assert!(
        policy["Policy"].as_str().unwrap().contains("sns-alerts"),
        "{policy}"
    );
    let (s, u) = op(
        &w,
        "echo",
        "CreateFunctionUrlConfig",
        &json!({"AuthType": "NONE", "Cors": {"AllowOrigins": ["*"], "MaxAge": 5}}),
    )
    .await;
    assert_eq!(s, 200, "{u}");
    let (_, d) = w.get(&format!("{FN}/echo")).await;
    assert_eq!(field(&d, "Function URL auth")["value"], "NONE");
    assert!(
        field(&d, "Function URL CORS")["value"]
            .as_str()
            .unwrap()
            .contains("AllowOrigins")
    );
    let (s, e) = op(
        &w,
        "echo",
        "PutFunctionEventInvokeConfig",
        &json!({"DestinationConfig": {"OnFailure": {"Destination": "arn:aws:sqs:us-east-1:000000000000:dead"}}}),
    )
    .await;
    assert_eq!(s, 200, "{e}");
    // The call is bound to this function whatever the page names.
    let (_, other) = op(
        &w,
        "echo",
        "GetPolicy",
        &json!({"FunctionName": "elsewhere"}),
    )
    .await;
    assert!(other["Policy"].is_string(), "{other}");
}

#[tokio::test]
async fn a_trigger_is_changed_only_by_the_function_it_belongs_to() {
    let w = world("lambda-triggers", Mode::Enforce, &[]);
    if !code::function(&w, "echo").await || !code::function_again(&w, "other").await {
        return;
    }
    w.app("sqs", "CreateQueue", &json!({"QueueName": "inbox"}))
        .await;
    let (s, made) = op(
        &w,
        "echo",
        "CreateEventSourceMapping",
        &json!({"EventSourceArn": "arn:aws:sqs:us-east-1:000000000000:inbox", "Enabled": true, "BatchSize": 10}),
    )
    .await;
    assert_eq!(s, 200, "{made}");
    let uuid = made["UUID"].as_str().unwrap();
    let (s, edited) = op(
        &w,
        "echo",
        "UpdateEventSourceMapping",
        &json!({"UUID": uuid, "BatchSize": 5}),
    )
    .await;
    assert_eq!(
        (s, edited["BatchSize"].clone()),
        (200, json!(5)),
        "{edited}"
    );
    let (s, refused) = op(
        &w,
        "other",
        "UpdateEventSourceMapping",
        &json!({"UUID": uuid, "BatchSize": 1}),
    )
    .await;
    assert_eq!(
        (s, refused["code"].clone()),
        (400, json!("NOT_THIS_RESOURCE")),
        "{refused}"
    );
    let (_, d) = w.get(&format!("{FN}/echo")).await;
    assert_eq!(firsts(sub(&d, "Event source mappings")), ["inbox"]);
}

#[tokio::test]
async fn a_delete_says_what_goes_with_the_function_and_what_stays() {
    let w = world("lambda-delete", Mode::Enforce, &[]);
    if !code::function(&w, "echo").await {
        return;
    }
    op(&w, "echo", "PublishVersion", &json!({})).await;
    // Lambda publishes a new version only when something changed since the last.
    let (s, c) = op(
        &w,
        "echo",
        "UpdateFunctionConfiguration",
        &json!({"MemorySize": 256}),
    )
    .await;
    assert_eq!(s, 200, "{c}");
    let (s, v2) = op(&w, "echo", "PublishVersion", &json!({})).await;
    assert_eq!((s, v2["Version"].clone()), (200, json!("2")), "{v2}");
    let (s, a) = op(
        &w,
        "echo",
        "CreateAlias",
        &json!({"Name": "live", "FunctionVersion": "2"}),
    )
    .await;
    assert_eq!(s, 200, "{a}");
    let (_, b) = w.get(&format!("{FN}/echo/blockers")).await;
    let titles: Vec<&str> = b["blockers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, ["Versions", "Aliases"]);
    assert_eq!(b["blockers"][0]["items"].as_array().unwrap().len(), 2);
    assert_eq!(b["words"], "confirm");
}

#[tokio::test]
async fn layers_are_published_listed_and_read_with_their_versions() {
    let w = world("lambda-layers", Mode::Enforce, &[]);
    if !code::function(&w, "echo").await {
        return;
    }
    let (_, none) = w.get("/api/v1/resources/lambda/layers").await;
    assert!(none["rows"].as_array().unwrap().is_empty());
    let zip = {
        let (_, _) = (0, 0);
        let res = tower::ServiceExt::oneshot(w.app.clone(), get(&format!("{FN}/echo/code")))
            .await
            .unwrap();
        http_body_util::BodyExt::collect(res.into_body())
            .await
            .unwrap()
            .to_bytes()
    };
    for description in ["one", "two"] {
        let (s, v) = w
            .send(
                "POST",
                "/api/v1/resources/lambda/layers/op/PublishLayerVersion",
                &json!({"LayerName": "larder-shared", "Description": description,
                    "Content": {"ZipFile": aws_core::base64::encode(&zip)},
                    "CompatibleRuntimes": ["python3.13"], "CompatibleArchitectures": ["x86_64"]}),
            )
            .await;
        assert_eq!(s, 200, "{v}");
    }
    let (_, list) = w.get("/api/v1/resources/lambda/layers").await;
    assert_eq!(list["rows"][0]["name"], "larder-shared");
    assert_eq!(list["rows"][0]["cells"][0], "2");
    assert_eq!(list["rows"][0]["cells"][1], "two");
    let (_, d) = w.get("/api/v1/resources/lambda/layers/larder-shared").await;
    assert_eq!(field(&d, "Latest version")["value"], "2");
    assert_eq!(sub(&d, "Versions")["rows"].as_array().unwrap().len(), 2);
    let (s, versions) = w
        .send(
            "POST",
            "/api/v1/resources/lambda/layers/op/ListLayerVersions",
            &json!({"LayerName": "larder-shared"}),
        )
        .await;
    assert_eq!(
        (s, versions["LayerVersions"].as_array().unwrap().len()),
        (200, 2)
    );
    let (s, gone) = w
        .send(
            "POST",
            "/api/v1/resources/lambda/layers/larder-shared/op/DeleteLayerVersion",
            &json!({"VersionNumber": 2}),
        )
        .await;
    assert_eq!(s, 200, "{gone}");
}

#[tokio::test]
async fn a_setting_a_function_never_had_is_an_answer_and_not_a_failed_request() {
    let w = world("lambda-absent", Mode::Enforce, &[]);
    if !code::function(&w, "echo").await {
        return;
    }
    for operation in [
        "GetPolicy",
        "GetFunctionEventInvokeConfig",
        "GetFunctionUrlConfig",
    ] {
        let (status, absent) = op(&w, "echo", operation, &json!({})).await;
        assert_eq!(status, 200, "{operation}: {absent}");
        assert_eq!(absent["Absent"], "ResourceNotFoundException", "{operation}");
    }
    // And a read the function has an answer for is the answer.
    op(&w, "echo", "PublishVersion", &json!({})).await;
    let (status, got) = op(&w, "echo", "GetFunctionConfiguration", &json!({})).await;
    assert_eq!((status, got["Absent"].clone()), (200, Value::Null));
}
