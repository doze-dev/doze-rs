//! `DozePrincipals`: the user behind each access key IAM issued, for the console's caller names. A read.

use std::sync::Arc;

use aws_wire::service::{Service, WebRequest};
use doze_runtime::{Ctx, SystemClock, Tasks};
use serde_json::{Value, json};

use super::{World, temp};

#[test]
fn an_issued_key_names_its_user_and_any_other_key_names_nobody() {
    let mut w = World::new();
    w.ok(
        "CreateUser",
        &json!({"UserName": "payments", "Path": "/apps/"}),
    );
    let made = w.ok("CreateAccessKey", &json!({"UserName": "payments"}));
    let key = made["AccessKey"]["AccessKeyId"]
        .as_str()
        .unwrap()
        .to_owned();
    let before = w.core.seen.len();
    let owners = w.core.owners(&[key, "AKIANOBODY0000000000".into()]);
    assert_eq!(
        owners,
        [
            Some((
                "payments".to_owned(),
                "arn:aws:iam::000000000000:user/apps/payments".to_owned()
            )),
            None
        ]
    );
    // Asking noted nothing: no last use, no access log row.
    assert_eq!(w.core.seen.len(), before);
}

fn web(query: &str) -> WebRequest {
    WebRequest {
        method: "GET".into(),
        host: "localhost:4566".into(),
        path: "/_doze/iam/DozePrincipals".into(),
        query: query.into(),
        headers: Vec::new(),
        body: Default::default(),
        endpoint: "http://localhost:4566".into(),
        signature: aws_wire::service::WebSignature::Unsigned,
    }
}

#[tokio::test]
async fn keys_iam_cannot_have_issued_are_answered_without_waking_it() {
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
    assert!(web_side.owns("localhost", "/_doze/iam/DozePrincipals"));
    let reply = web_side.serve(web("AccessKeyIds=test,console")).await;
    assert_eq!(reply.status, 200);
    let body: Value = serde_json::from_slice(&reply.body).unwrap();
    assert_eq!(body, json!({"Principals": []}));
    assert!(!iam.0.actor.is_live(), "no key asked about was IAM's");
    // A key that looks issued is looked up, and one nobody issued names nobody.
    let reply = web_side
        .serve(web("AccessKeyIds=AKIANOBODY0000000000"))
        .await;
    let body: Value = serde_json::from_slice(&reply.body).unwrap();
    assert_eq!(body, json!({"Principals": []}));
}
