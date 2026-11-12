//! State on disk: after every call of a broad scenario the service is stopped and started again, and what it loads is exactly
//! what it had. Key material sealed by the first run still opens after the restarts.

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
        let core = Core::open_in(dir, clock.clone()).unwrap();
        Self {
            w: World::with(core, clock),
            dir,
        }
    }

    fn step(&mut self, op: &str, request: &Value) -> Value {
        let answer = self.w.ok(op, request);
        let before = format!("{:?}\n{:?}", self.w.core.keys, self.w.core.aliases);
        if let Some(db) = self.w.core.db.take() {
            db.close();
        }
        self.w.core = Core::open_in(self.dir, self.w.clock.clone()).unwrap();
        assert_eq!(
            before,
            format!("{:?}\n{:?}", self.w.core.keys, self.w.core.aliases),
            "after {op} {request}"
        );
        answer
    }
}

#[test]
fn a_restart_changes_nothing_whatever_was_done_before_it() {
    let dir = temp();
    let mut r = Run::new(dir.path());
    let id = r.step(
        "CreateKey",
        &json!({"Description": "d", "Tags": [{"TagKey": "k", "TagValue": "v"}]}),
    )["KeyMetadata"]["KeyId"]
        .as_str()
        .unwrap()
        .to_owned();
    let sealed = r.step("Encrypt", &json!({"KeyId": id, "Plaintext": "eA=="}));
    r.step(
        "CreateAlias",
        &json!({"AliasName": "alias/a", "TargetKeyId": id}),
    );
    r.step(
        "EnableKeyRotation",
        &json!({"KeyId": id, "RotationPeriodInDays": 90}),
    );
    r.step("RotateKeyOnDemand", &json!({"KeyId": id}));
    r.step(
        "TagResource",
        &json!({"KeyId": id, "Tags": [{"TagKey": "t", "TagValue": "x"}]}),
    );
    r.step("PutKeyPolicy", &json!({"KeyId": id, "Policy": "{}"}));
    r.step("DisableKey", &json!({"KeyId": id}));
    r.step("EnableKey", &json!({"KeyId": id}));
    r.step(
        "CreateKey",
        &json!({"KeySpec": "ECC_NIST_P256", "KeyUsage": "SIGN_VERIFY"}),
    );
    r.step(
        "ScheduleKeyDeletion",
        &json!({"KeyId": id, "PendingWindowInDays": 7}),
    );
    r.step("CancelKeyDeletion", &json!({"KeyId": id}));
    r.step(
        "UpdateAlias",
        &json!({"AliasName": "alias/a", "TargetKeyId": id}),
    );
    r.step("DeleteAlias", &json!({"AliasName": "alias/a"}));
    r.step("EnableKey", &json!({"KeyId": id}));
    // Keys with imported material, and the wrapping key an import hands out, are kept too.
    let external = r.step("CreateKey", &json!({"Origin": "EXTERNAL"}))["KeyMetadata"]["KeyId"]
        .as_str()
        .unwrap()
        .to_owned();
    r.step("GetParametersForImport", &json!({"KeyId": external, "WrappingAlgorithm": "RSAES_OAEP_SHA_256", "WrappingKeySpec": "RSA_2048"}));
    r.step(
        "CreateKey",
        &json!({"KeySpec": "RSA_2048", "KeyUsage": "SIGN_VERIFY"}),
    );
    // Material sealed by the first run, rotated since, still opens.
    assert_eq!(
        r.w.ok(
            "Decrypt",
            &json!({"CiphertextBlob": sealed["CiphertextBlob"]})
        )["Plaintext"],
        "eA=="
    );
}

#[test]
fn a_call_writes_the_rows_it_changed_and_not_the_others() {
    let dir = temp();
    let mut r = Run::new(dir.path());
    let ids: Vec<String> = (0..10).map(|_| r.w.key(&json!({}))).collect();
    assert_eq!(r.w.rows_of("DisableKey", &json!({"KeyId": ids[3]})), 1);
    assert_eq!(
        r.w.rows_of("Encrypt", &json!({"KeyId": ids[4], "Plaintext": "eA=="})),
        0,
        "using a key changes nothing"
    );
    assert_eq!(
        r.w.rows_of(
            "CreateAlias",
            &json!({"AliasName": "alias/q", "TargetKeyId": ids[5]})
        ),
        1
    );
    assert_eq!(r.w.rows_of("DescribeKey", &json!({"KeyId": ids[6]})), 0);
}
