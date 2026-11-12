//! State on disk: after every call of a broad scenario the service is stopped and started again, and what it loads is exactly
//! what it had.

use serde_json::{Value, json};

use super::{START, World, temp};
use crate::core::Core;

struct Run<'a> {
    w: World,
    dir: &'a std::path::Path,
}

impl<'a> Run<'a> {
    fn new(dir: &'a std::path::Path) -> Self {
        let clock = std::sync::Arc::new(doze_runtime::ManualClock::at(START));
        let bus = super::Bus::new();
        let core = Core::open_in(dir, clock.clone(), Some(bus.peers.clone())).unwrap();
        Self {
            w: World::with(core, clock, bus),
            dir,
        }
    }

    fn step(&mut self, op: &str, request: &Value) -> Value {
        let answer = self.w.ok(op, request);
        let before = format!("{:?}", self.w.core.secrets);
        if let Some(db) = self.w.core.db.take() {
            db.close();
        }
        self.w.core = Core::open_in(
            self.dir,
            self.w.clock.clone(),
            Some(self.w.bus.peers.clone()),
        )
        .unwrap();
        assert_eq!(
            before,
            format!("{:?}", self.w.core.secrets),
            "after {op} {request}"
        );
        answer
    }
}

#[test]
fn a_restart_changes_nothing_whatever_was_done_before_it() {
    let dir = temp();
    let mut r = Run::new(dir.path());
    r.step("CreateSecret", &json!({"Name": "a", "SecretString": "one", "Description": "d", "Tags": [{"Key": "k", "Value": "v"}]}));
    r.step(
        "PutSecretValue",
        &json!({"SecretId": "a", "SecretString": "two"}),
    );
    r.step(
        "PutSecretValue",
        &json!({"SecretId": "a", "SecretBinary": "AAH+", "VersionStages": ["PENDING"]}),
    );
    r.step("GetSecretValue", &json!({"SecretId": "a"}));
    r.step(
        "TagResource",
        &json!({"SecretId": "a", "Tags": [{"Key": "t", "Value": "x"}]}),
    );
    r.step("UntagResource", &json!({"SecretId": "a", "TagKeys": ["k"]}));
    r.step(
        "PutResourcePolicy",
        &json!({"SecretId": "a", "ResourcePolicy": "{}"}),
    );
    r.step(
        "UpdateSecret",
        &json!({"SecretId": "a", "Description": "new"}),
    );
    r.step("CreateSecret", &json!({"Name": "b", "SecretString": "x"}));
    r.step(
        "DeleteSecret",
        &json!({"SecretId": "b", "RecoveryWindowInDays": 7}),
    );
    r.step("RestoreSecret", &json!({"SecretId": "b"}));
    r.step(
        "DeleteSecret",
        &json!({"SecretId": "b", "ForceDeleteWithoutRecovery": true}),
    );
    // The key made by the first run still opens the values the first run sealed.
    assert_eq!(
        r.w.ok("GetSecretValue", &json!({"SecretId": "a"}))["SecretString"],
        "two"
    );
}

#[test]
fn a_call_writes_the_rows_it_changed_and_not_the_others() {
    let dir = temp();
    let mut r = Run::new(dir.path());
    for n in 0..20 {
        r.w.create(&format!("many/{n}"), "v");
    }
    assert_eq!(
        r.w.rows_of(
            "PutSecretValue",
            &json!({"SecretId": "many/7", "SecretString": "x"})
        ),
        1
    );
    assert_eq!(
        r.w.rows_of("DescribeSecret", &json!({"SecretId": "many/7"})),
        0
    );
    assert_eq!(r.w.rows_of("ListSecrets", &json!({})), 0);
    assert_eq!(
        r.w.rows_of(
            "DeleteSecret",
            &json!({"SecretId": "many/9", "ForceDeleteWithoutRecovery": true})
        ),
        1
    );
}
