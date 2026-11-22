//! Home, Access, Lab and Settings against real services on a real bus: a caller named by IAM, a refusal grouped and explained,
//! a policy attached; the app's client read from its User-Agent, signals from the record and from the services; an operation
//! sent from the Lab and found in Live; the settings and the reset that needs the data engine.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;
#[path = "resources/world.rs"]
mod world;

use std::time::Duration;

use aws_gateway::{Did, Finished};
use aws_peers::Mode;
use aws_wire::service::Caller;
use serde_json::{Value, json};
use support::{keys, ts_keys};
use world::{World, world};

/// A request from the wire, as the gateway tells the record of it.
fn wire<'a>(
    key: &'a str,
    signing: &'static str,
    action: &'static str,
    ua: &'a str,
) -> Finished<'a> {
    Finished {
        request_id: "req",
        signing_name: signing,
        sdk_id: if signing == "sqs" { "SQS" } else { "IAM" },
        action,
        member: "",
        resource: "",
        access_key_id: key,
        region: "us-east-1",
        status: 200,
        code: "",
        took: Duration::from_micros(900),
        would_be_denied: false,
        user_agent: ua,
        message: "",
        function_error: "",
        answer_bytes: None,
        did: Did::NONE,
    }
}

fn caller(key: &str) -> Caller {
    Caller {
        access_key_id: key.to_owned(),
        account_id: "000000000000".to_owned(),
        region: "us-east-1".to_owned(),
        endpoint: "http://127.0.0.1:4566".to_owned(),
        service: None,
        session_token: None,
    }
}

/// An IAM user with no policy and an access key of its own.
async fn nobody(w: &World, name: &str) -> String {
    w.app("iam", "CreateUser", &json!({"UserName": name})).await;
    let made = w
        .app("iam", "CreateAccessKey", &json!({"UserName": name}))
        .await;
    made["AccessKey"]["AccessKeyId"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// The user `worker`, refused by soft IAM a ListQueues it was let through; the gateway saw it twice. Its key.
async fn refused_worker(w: &World) -> String {
    let key = nobody(w, "worker").await;
    // Soft IAM lets the call through and logs why it would not have.
    let peers = w.state.facts.peers.clone().unwrap();
    peers
        .call_as(&caller(&key), "sqs", "ListQueues", &json!({}))
        .await
        .unwrap();
    for _ in 0..2 {
        let refused = Finished {
            would_be_denied: true,
            ..wire(&key, "sqs", "ListQueues", "aws-cli/2.15.30")
        };
        w.state.recorder.record(&refused, 1);
    }
    key
}

#[tokio::test]
async fn a_refused_caller_is_named_grouped_explained_and_can_be_let_through() {
    let w = world("access", Mode::Soft, &[]);
    let key = refused_worker(&w).await;

    let (status, names) = w.get("/api/v1/principals").await;
    assert_eq!(status, 200);
    assert_eq!(keys(&names), ts_keys("PrincipalsResponse"));
    let named = names["principals"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["key"] == key.as_str())
        .unwrap()
        .clone();
    assert_eq!(keys(&named), ts_keys("KeyPrincipal"));
    assert_eq!(named["kind"], "user");
    assert_eq!(named["name"], "worker");
    assert_eq!(named["arn"], "arn:aws:iam::000000000000:user/worker");
    let kinds: Vec<&str> = names["principals"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["kind"].as_str().unwrap())
        .collect();
    assert!(
        kinds.contains(&"root"),
        "the app's key, which IAM did not issue: {names}"
    );

    let (status, view) = w.get("/api/v1/access").await;
    assert_eq!(status, 200, "{view}");
    assert_eq!(keys(&view), ts_keys("AccessView"));
    assert_eq!(view["log_read"], true);
    let groups = view["groups"].as_array().unwrap();
    assert_eq!(groups.len(), 1, "{view}");
    let g = &groups[0];
    assert_eq!(keys(g), ts_keys("DenialGroup"));
    assert_eq!(
        (g["action"].as_str(), g["service"].as_str()),
        (Some("ListQueues"), Some("sqs"))
    );
    assert_eq!(
        (g["would_be_denied"].as_u64(), g["denied"].as_u64()),
        (Some(2), Some(0))
    );
    assert_eq!(g["principal"]["name"], "worker");
    let why = &g["explained"][0];
    assert_eq!(keys(why), ts_keys("Refusal"));
    assert_eq!(why["action"], "sqs:ListQueues");
    assert_eq!(why["decision"], "implicitDeny");
    assert!(
        why["reason"]
            .as_str()
            .unwrap()
            .contains("no identity-based policy"),
        "{why}"
    );
    assert!(
        view["refusals"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["principal"] == "arn:aws:iam::000000000000:user/worker")
    );
}

#[tokio::test]
async fn make_this_pass_puts_the_drafted_policy_on_the_user_as_the_console() {
    let w = world("attach", Mode::Soft, &[]);
    nobody(&w, "worker").await;
    let document = json!({"Version": "2012-10-17", "Statement": [{"Effect": "Allow", "Action": ["sqs:ListQueues"], "Resource": "*"}]});
    let body = json!({"kind": "user", "name": "worker", "policy_name": "doze-drafted", "document": document.to_string()});
    let (status, done) = w.send("POST", "/api/v1/access/attach", &body).await;
    assert_eq!(status, 200, "{done}");
    assert_eq!(keys(&done), ts_keys("Attached"));
    let put = w
        .app(
            "iam",
            "GetUserPolicy",
            &json!({"UserName": "worker", "PolicyName": "doze-drafted"}),
        )
        .await;
    assert!(
        put["PolicyDocument"].as_str().unwrap().contains("sqs"),
        "{put}"
    );
    assert!(
        w.live().iter().any(|l| l == "console iam PutUserPolicy"),
        "{:?}",
        w.live()
    );
    // A role nobody made is IAM's refusal, in its words.
    let body = json!({"kind": "role", "name": "missing", "policy_name": "p", "document": "{}"});
    let (status, refused) = w.send("POST", "/api/v1/access/attach", &body).await;
    assert_eq!(status, 404, "{refused}");
    assert_eq!(refused["code"], "NoSuchEntity");
    let (status, _) = w
        .send("POST", "/api/v1/access/attach", &json!({"kind": "group"}))
        .await;
    assert_eq!(status, 400);
}

#[tokio::test]
async fn home_says_which_app_connected_with_which_client_and_what_needs_a_look() {
    let w = world("home", Mode::Soft, &[]);
    let (status, home) = w.get("/api/v1/home").await;
    assert_eq!(status, 200);
    assert_eq!(keys(&home), ts_keys("HomeView"));
    assert_eq!(home["connected"], false);

    w.state.recorder.record(
        &wire(
            "AKIAAPP",
            "sqs",
            "SendMessage",
            "Boto3/1.34.69 md/Botocore#1.34.69 ua/2.0",
        ),
        1,
    );
    for _ in 0..3 {
        let failed = Finished {
            status: 400,
            code: "QueueDoesNotExist",
            ..wire("AKIAAPP", "sqs", "GetQueueUrl", "")
        };
        w.state.recorder.record(&failed, 2);
    }
    let unbuilt = Finished {
        status: 501,
        code: "NotImplemented",
        ..wire("AKIAAPP", "sqs", "StartMessageMoveTask", "")
    };
    w.state.recorder.record(&unbuilt, 3);
    let (_, home) = w.get("/api/v1/home").await;
    assert_eq!(home["connected"], true);
    let who = &home["callers"][0];
    assert_eq!(keys(who), ts_keys("CallerSeen"));
    assert_eq!(who["principal"]["key"], "AKIAAPP");
    assert_eq!(who["sdks"], json!(["boto3 1.34.69"]));
    assert_eq!(who["calls"], 5);
    assert_eq!(home["services"][0]["service"], "sqs");
    assert_eq!(home["recent"].as_array().unwrap().len(), 5);
    assert_eq!(home["recent"][4]["sdk"], "boto3 1.34.69");
    let kinds: Vec<&str> = home["signals"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["not-built", "errors"], "{home}");
    let errors = &home["signals"][1];
    assert_eq!(keys(errors), ts_keys("Signal"));
    assert_eq!(errors["count"], 3);
    assert_eq!(errors["href"], "/live?outcome=error&service=sqs");
    assert!(
        errors["detail"]
            .as_str()
            .unwrap()
            .contains("QueueDoesNotExist")
    );
}

#[tokio::test]
async fn home_checks_find_a_dead_letter_queue_holding_messages() {
    let w = world("checks", Mode::Soft, &[]);
    // An app called SQS in this run, so it is looked in.
    w.state
        .recorder
        .record(&wire("AKIAAPP", "sqs", "SendMessage", ""), 1);
    let dlq = w
        .app("sqs", "CreateQueue", &json!({"QueueName": "jobs-dlq"}))
        .await["QueueUrl"]
        .as_str()
        .unwrap()
        .to_owned();
    let arn = "arn:aws:sqs:us-east-1:000000000000:jobs-dlq";
    let policy = json!({"deadLetterTargetArn": arn, "maxReceiveCount": "3"}).to_string();
    w.app(
        "sqs",
        "CreateQueue",
        &json!({"QueueName": "jobs", "Attributes": {"RedrivePolicy": policy}}),
    )
    .await;
    w.app(
        "sqs",
        "SendMessage",
        &json!({"QueueUrl": dlq, "MessageBody": "lost"}),
    )
    .await;
    let (status, checks) = w.get("/api/v1/home/checks").await;
    assert_eq!(status, 200, "{checks}");
    assert_eq!(keys(&checks), ts_keys("HomeChecks"));
    assert_eq!(checks["checked"], json!(["sqs"]));
    let found = &checks["signals"][0];
    assert_eq!(
        (found["kind"].as_str(), found["count"].as_u64()),
        (Some("dead-letters"), Some(1)),
        "{checks}"
    );
    assert_eq!(
        found["resource"],
        json!({"service": "sqs", "kind": "queues", "name": dlq})
    );
}

#[tokio::test]
async fn the_lab_sends_a_built_operation_and_finds_it_in_live() {
    let w = world("lab", Mode::Soft, &[]);
    let (status, index) = w.get("/api/v1/lab").await;
    assert_eq!(status, 200);
    assert_eq!(keys(&index), ts_keys("LabIndex"));
    let sqs = index["targets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["target"] == "sqs")
        .unwrap();
    assert_eq!(keys(sqs), ts_keys("LabTarget"));
    assert!(
        sqs["operations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|o| o == "CreateQueue")
    );

    let (status, op) = w.get("/api/v1/lab/sqs/CreateQueue").await;
    assert_eq!(status, 200, "{op}");
    assert_eq!(keys(&op), ts_keys("LabOperation"));
    let name = op["members"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["name"] == "QueueName")
        .unwrap();
    assert_eq!(
        (name["kind"].as_str(), name["required"].as_bool()),
        (Some("string"), Some(true))
    );
    let example: Value = serde_json::from_str(op["example"].as_str().unwrap()).unwrap();
    assert_eq!(example, json!({"QueueName": ""}));

    let (status, answer) = w
        .send(
            "POST",
            "/api/v1/lab/sqs/CreateQueue",
            &json!({"QueueName": "from-the-lab"}),
        )
        .await;
    assert_eq!(status, 200, "{answer}");
    assert_eq!(keys(&answer), ts_keys("LabAnswer"));
    assert!(answer["output"].as_str().unwrap().contains("from-the-lab"));
    let seq = answer["seq"].as_u64().unwrap();
    let call = w.state.recorder.get(seq).unwrap();
    assert_eq!((&*call.caller, call.action), ("console", "CreateQueue"));

    // The service's own refusal is the answer, not a failure of the Lab.
    let (status, answer) = w
        .send(
            "POST",
            "/api/v1/lab/sqs/GetQueueUrl",
            &json!({"QueueName": "missing"}),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(answer["error"]["code"], "QueueDoesNotExist", "{answer}");
    let (status, _) = w
        .send("POST", "/api/v1/lab/sqs/NoSuchThing", &json!({}))
        .await;
    assert_eq!(status, 404);
    let (status, _) = w
        .send("POST", "/api/v1/lab/sqs/CreateQueue", &json!([1]))
        .await;
    assert_eq!(status, 400);
}

#[tokio::test]
async fn settings_without_the_data_engine_say_so_and_refuse_a_reset() {
    let w = world("settings", Mode::Soft, &[]);
    let (status, view) = w.get("/api/v1/settings").await;
    assert_eq!(status, 200);
    assert_eq!(keys(&view), ts_keys("SettingsView"));
    assert_eq!(view["reset"], false);
    assert_eq!(view["data_bytes"], Value::Null);
    assert!(view["data_error"].as_str().unwrap().contains("data engine"));
    assert!(view["ring_most_bytes"].as_u64().unwrap() < 1 << 20);
    let (status, refused) = w
        .send("POST", "/api/v1/settings/reset", &json!({"dry_run": true}))
        .await;
    assert_eq!(status, 503, "{refused}");
    let (status, _) = w.send("POST", "/api/v1/settings/reset", &json!({})).await;
    assert_eq!(status, 400);
}
