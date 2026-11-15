//! `doze-aws data` on IAM: its file is reported, emptying is refused with a sentence, and a reset leaves IAM as a fresh data
//! directory has it: no identities, a new account signing key, and the file smaller.

use std::sync::Arc;

use aws_testkit::TempDir;
use doze_runtime::ManualClock;
use serde_json::json;

use super::{TRUST, World};
use crate::core::Core;
use crate::storage::{DataAsk, DataReply};

const START: i64 = 1_699_999_980_000;

fn reopen(w: &mut World, dir: &TempDir) {
    w.core.db.take().unwrap().close();
    w.core = Core::open_in(dir.path(), w.clock.clone()).unwrap();
}

#[test]
fn a_reset_leaves_iam_as_a_fresh_start_and_the_file_smaller() {
    let dir = TempDir::new("iam-data");
    let clock = Arc::new(ManualClock::at(START));
    let mut w = World::with(Core::open_in(dir.path(), clock.clone()).unwrap(), clock);
    let key_before = w.core.account().signing_key;
    let policy =
        json!({"Version": "2012-10-17", "Statement": [{"Effect": "Allow", "Action": "s3:GetObject",
        "Resource": (0..60).map(|n| format!("arn:aws:s3:::bucket-{n}/*")).collect::<Vec<_>>()}]})
        .to_string();
    for n in 0..200 {
        let role = format!("role-{n}");
        w.ok(
            "CreateRole",
            &json!({"RoleName": role, "AssumeRolePolicyDocument": TRUST}),
        );
        w.ok(
            "PutRolePolicy",
            &json!({"RoleName": role, "PolicyName": "p", "PolicyDocument": policy}),
        );
    }
    w.ok("CreateUser", &json!({"UserName": "u"}));
    w.ok("CreateAccessKey", &json!({"UserName": "u"}));

    let DataReply::Usage(file) = w.core.data(DataAsk::Usage) else {
        panic!()
    };
    assert!(file.bytes > 200 * 2000, "{file:?}");

    let DataReply::Emptied(dry) = w.core.data(DataAsk::Reset { dry_run: true }) else {
        panic!()
    };
    assert_eq!((dry.items, dry.freed), (202, 0));
    assert!(dry.bytes > 200 * 1000, "{dry:?}");
    assert_eq!(
        w.ok("ListRoles", &json!({"MaxItems": 1000}))["Roles"]
            .as_array()
            .unwrap()
            .len(),
        200
    );

    let DataReply::Emptied(done) = w.core.data(DataAsk::Reset { dry_run: false }) else {
        panic!()
    };
    assert!(done.freed > 0, "the file shrank: {done:?}");
    reopen(&mut w, &dir);
    assert!(
        w.ok("ListRoles", &json!({}))["Roles"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        w.ok("ListUsers", &json!({}))["Users"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_ne!(w.core.account().signing_key, key_before, "a fresh account");
    // Names are free at once.
    w.ok(
        "CreateRole",
        &json!({"RoleName": "role-0", "AssumeRolePolicyDocument": TRUST}),
    );
}
