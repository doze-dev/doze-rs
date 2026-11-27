//! Which identity policy let a request through: the first Allow statement that matched, in the order the principal's policies are
//! evaluated (its own inline policies by name, its own attached policies by ARN, then each group, by name, the same way).

use aws_peers::{Mode, Need};
use aws_wire::service::Caller;
use serde_json::json;

use super::World;

const QUEUE: &str = "arn:aws:sqs:us-east-1:000000000000:jobs";
const SEND: &str = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Action":"sqs:SendMessage","Resource":"arn:aws:sqs:*:*:jobs"}]}"#;
const DENY: &str =
    r#"{"Version":"2012-10-17","Statement":[{"Effect":"Deny","Action":"sqs:*","Resource":"*"}]}"#;

fn caller(key: &str) -> Caller {
    Caller {
        access_key_id: key.into(),
        account_id: "000000000000".into(),
        region: "us-east-1".into(),
        endpoint: "http://localhost:4566".into(),
        service: None,
        session_token: None,
    }
}

fn user_with_key(w: &mut World, name: &str) -> String {
    w.ok("CreateUser", &json!({"UserName": name}));
    w.ok("CreateAccessKey", &json!({"UserName": name}))["AccessKey"]["AccessKeyId"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// What the evaluator names for `sqs:SendMessage` by `key`: the policy and its kind, when it names one.
fn named(w: &mut World, key: &str) -> Option<(String, &'static str)> {
    w.core.seen.clear();
    w.core
        .authorize(&caller(key), &[Need::new("sqs:SendMessage", QUEUE)])
        .ok();
    assert_eq!(w.core.seen.len(), 1);
    w.core.seen[0].by.clone()
}

fn managed(w: &mut World, name: &str, document: &str) -> String {
    w.ok(
        "CreatePolicy",
        &json!({"PolicyName": name, "PolicyDocument": document}),
    )["Policy"]["Arn"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn an_inline_policy_that_allows_is_named_with_its_kind() {
    let mut w = World::new();
    w.core.mode = Mode::Enforce;
    let key = user_with_key(&mut w, "alice");
    assert_eq!(named(&mut w, &key), None, "no policy allows it yet");
    w.ok(
        "PutUserPolicy",
        &json!({"UserName": "alice", "PolicyName": "send", "PolicyDocument": SEND}),
    );
    assert_eq!(named(&mut w, &key), Some(("send".into(), "user")));
}

#[test]
fn a_managed_policy_is_named_by_its_name_and_says_who_made_it() {
    let mut w = World::new();
    w.core.mode = Mode::Soft;
    let key = user_with_key(&mut w, "bob");
    let arn = managed(&mut w, "queue-writer", SEND);
    w.ok(
        "AttachUserPolicy",
        &json!({"UserName": "bob", "PolicyArn": arn}),
    );
    assert_eq!(
        named(&mut w, &key),
        Some(("queue-writer".into(), "user-managed"))
    );
    w.ok(
        "AttachUserPolicy",
        &json!({"UserName": "bob", "PolicyArn": "arn:aws:iam::aws:policy/PowerUserAccess"}),
    );
    // Attached policies are evaluated by ARN: `arn:aws:iam::000000000000:...` before `arn:aws:iam::aws:...`.
    assert_eq!(
        named(&mut w, &key),
        Some(("queue-writer".into(), "user-managed"))
    );
    w.ok(
        "DetachUserPolicy",
        &json!({"UserName": "bob", "PolicyArn": arn}),
    );
    assert_eq!(
        named(&mut w, &key),
        Some(("PowerUserAccess".into(), "aws-managed"))
    );
}

#[test]
fn when_several_policies_allow_the_one_evaluated_first_is_named() {
    let mut w = World::new();
    w.core.mode = Mode::Enforce;
    let key = user_with_key(&mut w, "carol");
    w.ok("CreateGroup", &json!({"GroupName": "writers"}));
    w.ok(
        "AddUserToGroup",
        &json!({"UserName": "carol", "GroupName": "writers"}),
    );
    w.ok(
        "PutGroupPolicy",
        &json!({"GroupName": "writers", "PolicyName": "from-group", "PolicyDocument": SEND}),
    );
    // Only the group allows: the group's policy is named, as a group's.
    assert_eq!(named(&mut w, &key), Some(("from-group".into(), "group")));
    // The user's own policies come before its groups', whatever the names sort to.
    let arn = managed(&mut w, "zz-attached", SEND);
    w.ok(
        "AttachUserPolicy",
        &json!({"UserName": "carol", "PolicyArn": arn}),
    );
    assert_eq!(
        named(&mut w, &key),
        Some(("zz-attached".into(), "user-managed"))
    );
    // And an inline policy comes before an attached one.
    w.ok(
        "PutUserPolicy",
        &json!({"UserName": "carol", "PolicyName": "zzz-inline", "PolicyDocument": SEND}),
    );
    assert_eq!(named(&mut w, &key), Some(("zzz-inline".into(), "user")));
    // Of two inline policies, the name that sorts first.
    w.ok(
        "PutUserPolicy",
        &json!({"UserName": "carol", "PolicyName": "aaa-inline", "PolicyDocument": SEND}),
    );
    assert_eq!(named(&mut w, &key), Some(("aaa-inline".into(), "user")));
}

#[test]
fn a_policy_that_does_not_match_is_not_named() {
    let mut w = World::new();
    w.core.mode = Mode::Enforce;
    let key = user_with_key(&mut w, "dave");
    let other = SEND.replace("sqs:SendMessage", "sqs:DeleteQueue");
    w.ok(
        "PutUserPolicy",
        &json!({"UserName": "dave", "PolicyName": "aaa-other", "PolicyDocument": other}),
    );
    w.ok(
        "PutUserPolicy",
        &json!({"UserName": "dave", "PolicyName": "zzz-send", "PolicyDocument": SEND}),
    );
    assert_eq!(named(&mut w, &key), Some(("zzz-send".into(), "user")));
}

#[test]
fn a_call_with_an_explicit_deny_or_no_allow_names_nothing() {
    let mut w = World::new();
    w.core.mode = Mode::Enforce;
    let key = user_with_key(&mut w, "erin");
    w.ok(
        "PutUserPolicy",
        &json!({"UserName": "erin", "PolicyName": "send", "PolicyDocument": SEND}),
    );
    w.ok(
        "PutUserPolicy",
        &json!({"UserName": "erin", "PolicyName": "deny", "PolicyDocument": DENY}),
    );
    assert_eq!(
        named(&mut w, &key),
        None,
        "the deny wins, and the allow is not credited"
    );
    assert_eq!(w.core.seen[0].decision, "explicitDeny");
}

#[test]
fn the_root_and_off_name_nothing() {
    let mut w = World::new();
    w.core.mode = Mode::Enforce;
    // A key IAM did not issue is the account root's.
    assert_eq!(named(&mut w, "test"), None);
    assert_eq!(w.core.seen[0].decision, "allowed");
    // With IAM off nothing is asked, so nothing is kept.
    let key = user_with_key(&mut w, "frank");
    w.ok(
        "PutUserPolicy",
        &json!({"UserName": "frank", "PolicyName": "send", "PolicyDocument": SEND}),
    );
    w.core.mode = Mode::Off;
    w.core.seen.clear();
    assert!(
        w.core
            .authorize(&caller(&key), &[Need::new("sqs:SendMessage", QUEUE)])
            .is_ok()
    );
    assert!(w.core.seen.is_empty());
}

/// A bus that remembers what it is told of allowing policies, and IAM over it.
fn told_bus() -> (crate::Iam, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
    use std::sync::{Arc, Mutex};

    use doze_runtime::{Ctx, SystemClock, Tasks};

    let peers = Arc::new(aws_peers::Peers::default());
    peers.set_mode(Mode::Soft);
    let told: Arc<Mutex<Vec<String>>> = Arc::default();
    let sink = Arc::clone(&told);
    peers.on_allowed_by(move |c, action, policy, kind| {
        sink.lock()
            .unwrap()
            .push(format!("{} {action} {policy} {kind}", c.access_key_id));
    });
    let ctx = Ctx {
        dir: std::env::temp_dir().join("doze-aws-iam-unused"),
        idle: std::time::Duration::from_secs(300),
        tasks: Tasks::new(),
        clock: Arc::new(SystemClock),
        ext: Some(peers),
    };
    (crate::services(Arc::new(ctx)).0, told)
}

fn seen(
    action: &str,
    decision: &'static str,
    by: Option<(&str, &'static str)>,
) -> crate::accesslog::Seen {
    crate::accesslog::Seen {
        principal: "p".into(),
        role: None,
        action: action.into(),
        resource: "*".into(),
        decision,
        reason: String::new(),
        by: by.map(|(n, k)| (n.to_owned(), k)),
        at_ms: 0,
    }
}

#[test]
fn the_bus_is_told_the_first_policy_only_when_every_question_was_allowed() {
    let (iam, told) = told_bus();
    let c = caller("AKIAX");
    let ok: Result<(), aws_wire::ApiError> = Ok(());
    // Two questions, both allowed: the first policy named is told, with the action of its question.
    iam.0.tell_allowed_by(
        &c,
        &ok,
        &[
            seen("s3:GetObject", "allowed", None),
            seen(
                "s3:PutObject",
                "allowed",
                Some(("PowerUserAccess", "aws-managed")),
            ),
            seen("s3:ListBucket", "allowed", Some(("other", "user"))),
        ],
    );
    // Soft mode let one question through that the policies refuse: the call is not credited to a policy.
    iam.0.tell_allowed_by(
        &c,
        &ok,
        &[
            seen("s3:GetObject", "allowed", Some(("a", "user"))),
            seen("s3:PutObject", "implicitDeny", None),
        ],
    );
    // A refusal is told nothing.
    let refused = Err(aws_wire::ApiError::new(400, "AccessDeniedException", "no"));
    let one = [seen("s3:GetObject", "allowed", Some(("a", "user")))];
    iam.0.tell_allowed_by(&c, &refused, &one);
    assert_eq!(
        *told.lock().unwrap(),
        ["AKIAX s3:PutObject PowerUserAccess aws-managed"]
    );
}
