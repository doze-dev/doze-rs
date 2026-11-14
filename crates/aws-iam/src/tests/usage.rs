//! What enforcement needs beyond the evaluator: AWS-managed policies with documents, `aws:SecureTransport`, the last use of a key
//! or role, and doze's access log and policy generator.

use std::sync::Arc;

use aws_peers::{Mode, Need};
use aws_wire::service::{Caller, WebRequest};
use doze_runtime::{Ctx, SystemClock, Tasks};
use serde_json::{Value, json};

use super::{START, TRUST, World, temp};
use crate::core::Core;
use crate::model::{Managed, Version};

fn caller(key: &str, token: Option<&str>) -> Caller {
    Caller {
        access_key_id: key.into(),
        account_id: "000000000000".into(),
        region: "eu-west-2".into(),
        endpoint: "http://localhost:4566".into(),
        service: None,
        session_token: token.map(str::to_owned),
    }
}

const QUEUE: &str = "arn:aws:sqs:eu-west-2:000000000000:jobs";

/// A role with `policy` attached, assumed: the session's caller.
fn session_with(w: &mut World, role: &str, policy: &str) -> Caller {
    w.ok(
        "CreateRole",
        &json!({"RoleName": role, "AssumeRolePolicyDocument": TRUST}),
    );
    w.ok(
        "AttachRolePolicy",
        &json!({"RoleName": role, "PolicyArn": policy}),
    );
    let assumed = w.sts_ok(
        "AssumeRole",
        &json!({"RoleArn": format!("arn:aws:iam::000000000000:role/{role}"), "RoleSessionName": "ss"}),
    );
    caller(
        assumed["Credentials"]["AccessKeyId"].as_str().unwrap(),
        assumed["Credentials"]["SessionToken"].as_str(),
    )
}

fn allowed(w: &mut World, c: &Caller, action: &str, resource: &str) -> bool {
    w.core.authorize(c, &[Need::new(action, resource)]).is_ok()
}

#[test]
fn an_aws_managed_policy_allows_what_its_name_says() {
    let mut w = World::new();
    let c = session_with(
        &mut w,
        "worker",
        "arn:aws:iam::aws:policy/service-role/AWSLambdaSQSQueueExecutionRole",
    );
    w.core.mode = Mode::Enforce;
    assert!(allowed(&mut w, &c, "sqs:ReceiveMessage", QUEUE));
    assert!(allowed(&mut w, &c, "logs:PutLogEvents", "*"));
    assert!(!allowed(&mut w, &c, "sqs:SendMessage", QUEUE));

    w.core.mode = Mode::Soft;
    let c = session_with(&mut w, "reader", "arn:aws:iam::aws:policy/ReadOnlyAccess");
    w.core.mode = Mode::Enforce;
    assert!(allowed(&mut w, &c, "dynamodb:GetItem", "*"));
    assert!(allowed(&mut w, &c, "s3:ListBucket", "*"));
    assert!(!allowed(&mut w, &c, "s3:PutObject", "*"));

    w.core.mode = Mode::Soft;
    let c = session_with(&mut w, "power", "arn:aws:iam::aws:policy/PowerUserAccess");
    w.core.mode = Mode::Enforce;
    assert!(allowed(&mut w, &c, "sqs:DeleteQueue", QUEUE));
    assert!(!allowed(&mut w, &c, "iam:CreateUser", "*"));
    assert!(allowed(&mut w, &c, "iam:CreateServiceLinkedRole", "*"));
}

#[test]
fn an_aws_managed_name_the_rules_do_not_know_is_not_a_policy_and_a_known_one_can_be_read_unattached()
 {
    let mut w = World::new();
    w.ok(
        "CreateRole",
        &json!({"RoleName": "r", "AssumeRolePolicyDocument": TRUST}),
    );
    let (code, message) = w.err(
        "AttachRolePolicy",
        &json!({"RoleName": "r", "PolicyArn": "arn:aws:iam::aws:policy/AmazonMadeUpFullAccess"}),
    );
    assert_eq!(code, "NoSuchEntityException");
    assert_eq!(
        message,
        "Policy arn:aws:iam::aws:policy/AmazonMadeUpFullAccess was not found."
    );
    // AWS's policies exist whether or not the account has used one.
    let arn = "arn:aws:iam::aws:policy/AmazonS3ReadOnlyAccess";
    let got = w.ok("GetPolicy", &json!({"PolicyArn": arn}))["Policy"].clone();
    assert_eq!(got["PolicyName"], "AmazonS3ReadOnlyAccess");
    assert_eq!(got["AttachmentCount"], 0);
    let version = w.ok(
        "GetPolicyVersion",
        &json!({"PolicyArn": arn, "VersionId": "v1"}),
    );
    let text =
        aws_core::uri::decode_str(version["PolicyVersion"]["Document"].as_str().unwrap()).unwrap();
    assert!(text.contains("s3:Get*"), "{text}");
    // Attaching keeps it, with the same id it was shown with before.
    w.ok(
        "AttachRolePolicy",
        &json!({"RoleName": "r", "PolicyArn": arn}),
    );
    let kept = w.ok("GetPolicy", &json!({"PolicyArn": arn}))["Policy"].clone();
    assert_eq!(kept["PolicyId"], got["PolicyId"]);
    assert_eq!(kept["AttachmentCount"], 1);
    // An AWS-managed policy can be a boundary.
    w.ok(
        "PutRolePermissionsBoundary",
        &json!({"RoleName": "r", "PermissionsBoundary": "arn:aws:iam::aws:policy/PowerUserAccess"}),
    );
}

#[test]
fn an_aws_managed_policy_kept_empty_before_is_given_its_document_when_opened() {
    let dir = temp();
    let clock = Arc::new(doze_runtime::ManualClock::at(START));
    let arn = "arn:aws:iam::aws:policy/AmazonSQSFullAccess";
    {
        let mut core = Core::open_in(dir.path(), clock.clone()).unwrap();
        core.policies.insert(
            arn.to_owned(),
            Managed {
                arn: arn.to_owned(),
                name: "AmazonSQSFullAccess".into(),
                path: "/".into(),
                id: "ANPAOLD".into(),
                description: None,
                created_ms: START,
                updated_ms: START,
                versions: vec![Version {
                    id: "v1".into(),
                    document: r#"{"Version":"2012-10-17","Statement":[]}"#.into(),
                    created_ms: START,
                }],
                default: "v1".into(),
                next_version: 2,
                tags: std::collections::BTreeMap::new(),
                aws_managed: true,
            },
        );
        core.flush().unwrap().unwrap().wait().unwrap();
        core.db.take().unwrap().close();
    }
    let core = Core::open_in(dir.path(), clock).unwrap();
    let p = core.policies.get(arn).unwrap();
    assert!(p.versions[0].document.contains("sqs:*"), "{:?}", p.versions);
}

#[test]
fn secure_transport_is_true_so_an_enforce_ssl_policy_does_not_refuse_local_traffic() {
    let mut w = World::new();
    w.ok("CreateUser", &json!({"UserName": "u"}));
    let key = w.ok("CreateAccessKey", &json!({"UserName": "u"}))["AccessKey"]["AccessKeyId"]
        .as_str()
        .unwrap()
        .to_owned();
    w.core.mode = Mode::Enforce;
    let c = caller(&key, None);
    // CDK's `enforceSSL` statement, as a queue policy, beside an allow that names the user.
    let policy = r#"{"Version":"2012-10-17","Statement":[
        {"Effect":"Deny","Principal":{"AWS":"*"},"Action":"sqs:*","Resource":"*","Condition":{"Bool":{"aws:SecureTransport":"false"}}},
        {"Effect":"Allow","Principal":{"AWS":"arn:aws:iam::000000000000:user/u"},"Action":"sqs:SendMessage","Resource":"*"}]}"#;
    let need = Need::new("sqs:SendMessage", QUEUE).with_policy(Some(policy.to_owned()));
    assert!(w.core.authorize(&c, &[need]).is_ok());
}

#[test]
fn a_keys_last_use_is_noted_at_most_once_a_minute() {
    let mut w = World::new();
    w.ok("CreateUser", &json!({"UserName": "u"}));
    let key = w.ok("CreateAccessKey", &json!({"UserName": "u"}))["AccessKey"]["AccessKeyId"]
        .as_str()
        .unwrap()
        .to_owned();
    let never = w.ok("GetAccessKeyLastUsed", &json!({"AccessKeyId": key}));
    assert_eq!(never["AccessKeyLastUsed"]["ServiceName"], "N/A");

    let c = caller(&key, None);
    w.core
        .authorize(&c, &[Need::new("sqs:SendMessage", QUEUE)])
        .unwrap();
    let used =
        w.ok("GetAccessKeyLastUsed", &json!({"AccessKeyId": key}))["AccessKeyLastUsed"].clone();
    assert_eq!(used["ServiceName"], "sqs");
    assert_eq!(used["Region"], "eu-west-2");
    assert!(used["LastUsedDate"].is_number(), "{used}");
    w.core.keys.stage().unwrap();

    // Within the minute: nothing is written.
    w.clock.advance(30_000);
    w.core
        .authorize(&c, &[Need::new("s3:GetObject", "*")])
        .unwrap();
    assert_eq!(w.core.keys.stage().unwrap().rows(), 0);
    // After it: the new use is.
    w.clock.advance(31_000);
    w.core
        .authorize(&c, &[Need::new("s3:GetObject", "*")])
        .unwrap();
    assert_eq!(w.core.keys.stage().unwrap().rows(), 1);
    let used =
        w.ok("GetAccessKeyLastUsed", &json!({"AccessKeyId": key}))["AccessKeyLastUsed"].clone();
    assert_eq!(used["ServiceName"], "s3");
}

#[test]
fn a_roles_last_use_is_noted_from_its_sessions() {
    let mut w = World::new();
    let c = session_with(&mut w, "r", "arn:aws:iam::aws:policy/AdministratorAccess");
    assert_eq!(
        w.ok("GetRole", &json!({"RoleName": "r"}))["Role"].get("RoleLastUsed"),
        None
    );
    w.core
        .authorize(&c, &[Need::new("sqs:SendMessage", QUEUE)])
        .unwrap();
    let last = w.ok("GetRole", &json!({"RoleName": "r"}))["Role"]["RoleLastUsed"].clone();
    assert_eq!(last["Region"], "eu-west-2");
    assert!(last["LastUsedDate"].is_number(), "{last}");
}

#[test]
fn every_question_is_kept_for_the_access_log_with_its_reason() {
    let mut w = World::new();
    let c = session_with(
        &mut w,
        "worker",
        "arn:aws:iam::aws:policy/service-role/AWSLambdaSQSQueueExecutionRole",
    );
    w.core.seen.clear();
    w.core
        .authorize(
            &c,
            &[
                Need::new("sqs:ReceiveMessage", QUEUE),
                Need::new("sqs:SendMessage", QUEUE),
            ],
        )
        .unwrap();
    let seen = std::mem::take(&mut w.core.seen);
    assert_eq!(seen.len(), 2);
    assert_eq!(
        seen[0].principal,
        "arn:aws:sts::000000000000:assumed-role/worker/ss"
    );
    assert_eq!(
        seen[0].role.as_deref(),
        Some("arn:aws:iam::000000000000:role/worker")
    );
    assert_eq!(seen[0].decision, "allowed");
    assert_eq!(
        seen[0].reason,
        "allowed by statement 1 of AWSLambdaSQSQueueExecutionRole (aws-managed)"
    );
    assert_eq!(seen[1].decision, "implicitDeny");
    assert_eq!(
        seen[1].reason,
        "because no identity-based policy allows the sqs:SendMessage action"
    );
    // With IAM off nothing is asked, so nothing is kept.
    w.core.mode = Mode::Off;
    w.core
        .authorize(&c, &[Need::new("sqs:SendMessage", QUEUE)])
        .unwrap();
    assert!(w.core.seen.is_empty());
}

fn web(path: &str, query: &str) -> WebRequest {
    WebRequest {
        method: "GET".into(),
        host: "localhost:4566".into(),
        path: path.into(),
        query: query.into(),
        headers: Vec::new(),
        body: Default::default(),
        endpoint: "http://localhost:4566".into(),
    }
}

fn body(reply: &aws_wire::service::WebReply) -> Value {
    serde_json::from_slice(&reply.body).unwrap()
}

#[tokio::test]
async fn the_root_is_answered_without_waking_iam_and_the_log_and_generator_are_served() {
    use aws_peers::Authorizer;
    use aws_wire::service::Service;
    let dir = temp();
    let ctx = Ctx {
        dir: dir.path().to_owned(),
        idle: std::time::Duration::from_secs(300),
        tasks: Tasks::new(),
        clock: Arc::new(SystemClock),
        ext: None,
    };
    let (iam, _sts) = crate::services(Arc::new(ctx));
    let web_side = iam.web().unwrap();
    assert!(web_side.owns("localhost", "/_doze/iam/DozeAccessLog"));
    assert!(!web_side.owns("localhost", "/"));
    // Nothing asked yet: no log, and the generator says so.
    let empty = web_side
        .serve(web("/_doze/iam/DozeGeneratePolicy", ""))
        .await;
    assert_eq!(empty.status, 404);
    assert!(iam.0.log.lock().unwrap().is_none());

    let root = caller("test", None);
    iam.authorize(
        &root,
        vec![
            Need::new("sqs:SendMessage", QUEUE),
            Need::new("sqs:ListQueues", "*"),
        ],
    )
    .await
    .unwrap();
    iam.authorize(&root, vec![Need::new("sqs:SendMessage", QUEUE)])
        .await
        .unwrap();
    assert!(
        !iam.0.actor.is_live(),
        "a key IAM did not issue does not wake it"
    );

    let log = body(&web_side.serve(web("/_doze/iam/DozeAccessLog", "")).await);
    assert_eq!(log["Mode"], "soft");
    let entries = log["Entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2);
    let send = entries
        .iter()
        .find(|e| e["Action"] == "sqs:SendMessage")
        .unwrap();
    assert_eq!(send["Count"], 2);
    assert_eq!(send["Principal"], "arn:aws:iam::000000000000:root");
    assert_eq!(send["Decision"], "allowed");

    let policy = body(
        &web_side
            .serve(web(
                "/_doze/iam/DozeGeneratePolicy",
                "Principal=arn%3Aaws%3Aiam%3A%3A000000000000%3Aroot&ScopeToResources=true",
            ))
            .await,
    );
    assert_eq!(policy["StatementCount"], 2);
    assert_eq!(policy["UnresolvedResources"], 1);
    assert_eq!(
        policy["PolicyDocument"]["Statement"][1],
        json!({"Effect": "Allow", "Action": ["sqs:SendMessage"], "Resource": QUEUE})
    );

    let reset = body(
        &web_side
            .serve(web("/_doze/iam/DozeAccessLog", "Reset=true"))
            .await,
    );
    assert_eq!(reset["Entries"], json!([]));
    let unknown = web_side.serve(web("/_doze/iam/Other", "")).await;
    assert_eq!(unknown.status, 404);
    let _ = iam.meta();
}
