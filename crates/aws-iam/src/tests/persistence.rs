//! State on disk: after every call of a broad scenario the service is stopped and started again, and what it loads is exactly what it
//! had. Sessions made before a restart are still recognised after it (the signing key is kept).

use serde_json::{Value, json};

use super::{READ_S3, START, TRUST, World, temp};
use crate::core::Core;

struct Run<'a> {
    w: World,
    dir: &'a std::path::Path,
}

fn state(c: &Core) -> String {
    format!(
        "{:?}\n{:?}\n{:?}\n{:?}\n{:?}\n{:?}",
        c.entities,
        c.policies,
        c.keys,
        c.profiles,
        c.providers,
        c.account()
    )
}

impl<'a> Run<'a> {
    fn new(dir: &'a std::path::Path) -> Self {
        let clock = std::sync::Arc::new(doze_runtime::ManualClock::at(START));
        let core = Core::open_in(dir, clock.clone()).unwrap();
        Self {
            w: World::with(core, clock),
            dir,
        }
    }

    fn step(&mut self, op: &str, request: &Value) -> Value {
        let answer = self.w.ok(op, request);
        let before = state(&self.w.core);
        if let Some(db) = self.w.core.db.take() {
            db.close();
        }
        self.w.core = Core::open_in(self.dir, self.w.clock.clone()).unwrap();
        assert_eq!(before, state(&self.w.core), "after {op} {request}");
        answer
    }
}

#[test]
fn a_restart_changes_nothing_whatever_was_done_before_it() {
    let dir = temp();
    let mut r = Run::new(dir.path());
    r.step("CreateRole", &json!({"RoleName": "r", "AssumeRolePolicyDocument": TRUST, "Tags": [{"Key": "k", "Value": "v"}]}));
    r.step("CreateUser", &json!({"UserName": "u"}));
    r.step("CreateGroup", &json!({"GroupName": "g"}));
    r.step(
        "AddUserToGroup",
        &json!({"GroupName": "g", "UserName": "u"}),
    );
    r.step("CreateAccessKey", &json!({"UserName": "u"}));
    r.step(
        "PutRolePolicy",
        &json!({"RoleName": "r", "PolicyName": "p", "PolicyDocument": READ_S3}),
    );
    let arn = r.step(
        "CreatePolicy",
        &json!({"PolicyName": "m", "PolicyDocument": READ_S3}),
    )["Policy"]["Arn"]
        .as_str()
        .unwrap()
        .to_owned();
    r.step(
        "AttachRolePolicy",
        &json!({"RoleName": "r", "PolicyArn": arn}),
    );
    r.step(
        "CreatePolicyVersion",
        &json!({"PolicyArn": arn, "PolicyDocument": READ_S3, "SetAsDefault": true}),
    );
    r.step(
        "CreateInstanceProfile",
        &json!({"InstanceProfileName": "ip"}),
    );
    r.step(
        "AddRoleToInstanceProfile",
        &json!({"InstanceProfileName": "ip", "RoleName": "r"}),
    );
    r.step("CreateOpenIDConnectProvider", &json!({"Url": "https://x.example", "ThumbprintList": ["0123456789012345678901234567890123456789"]}));
    r.step("CreateAccountAlias", &json!({"AccountAlias": "acme"}));
    r.step("UpdateUser", &json!({"UserName": "u", "NewUserName": "v"}));
    r.step(
        "DetachRolePolicy",
        &json!({"RoleName": "r", "PolicyArn": arn}),
    );
    r.step(
        "RemoveRoleFromInstanceProfile",
        &json!({"InstanceProfileName": "ip", "RoleName": "r"}),
    );
    // A session made before a restart is recognised after it.
    let got = r.w.sts_ok(
        "AssumeRole",
        &json!({"RoleArn": "arn:aws:iam::000000000000:role/r", "RoleSessionName": "sess"}),
    );
    r.w.core.db.take().unwrap().close();
    r.w.core = Core::open_in(r.dir, r.w.clock.clone()).unwrap();
    r.w.sts.session_token = got["Credentials"]["SessionToken"]
        .as_str()
        .map(str::to_owned);
    assert_eq!(
        r.w.sts_ok("GetCallerIdentity", &json!({}))["Arn"],
        "arn:aws:sts::000000000000:assumed-role/r/sess"
    );
}

#[test]
fn a_call_writes_the_rows_it_changed_and_not_the_others() {
    let dir = temp();
    let mut r = Run::new(dir.path());
    for n in 0..10 {
        r.w.ok("CreateUser", &json!({"UserName": format!("u{n}")}));
    }
    assert_eq!(
        r.w.rows_of(
            "TagUser",
            &json!({"UserName": "u3", "Tags": [{"Key": "a", "Value": "b"}]})
        ),
        1
    );
    assert_eq!(r.w.rows_of("GetUser", &json!({"UserName": "u4"})), 0);
    assert_eq!(r.w.rows_of("ListUsers", &json!({})), 0);
}
