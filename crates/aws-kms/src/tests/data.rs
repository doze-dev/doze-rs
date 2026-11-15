//! `doze-aws data` on KMS: its file is reported, emptying is refused, and a reset leaves KMS as a fresh data directory has it: no
//! keys, no aliases (the AWS-managed `alias/aws/ssm` included, made again when SSM next asks), and a new sealing key.

use std::sync::Arc;

use aws_testkit::Harness;
use doze_runtime::{Ctx, ManualClock, SystemClock, Tasks};
use serde_json::json;

use super::{START, World, temp};
use crate::Kms;
use crate::core::Core;
use crate::storage::{DataAsk, DataReply};

fn reopen(w: &mut World, dir: &std::path::Path) {
    w.core.db.take().unwrap().close();
    w.core = Core::open_in(dir, w.clock.clone()).unwrap();
}

/// SSM's first use of its AWS-managed key.
fn ssm_encrypts(w: &mut World) {
    let h = Harness::new(Kms::new(Arc::new(Ctx {
        dir: std::env::temp_dir().join("doze-aws-kms-unused"),
        idle: std::time::Duration::from_secs(300),
        tasks: Tasks::new(),
        clock: Arc::new(SystemClock),
        ext: None,
    })));
    let ix = h.op("Encrypt");
    let mut call = h
        .call(ix, &json!({"KeyId": "alias/aws/ssm", "Plaintext": "aGk="}))
        .unwrap();
    call.caller.service = Some("ssm".into());
    w.core.dispatch(&call).unwrap();
    w.core.flush().unwrap().unwrap().wait().unwrap();
}

fn aliases(w: &mut World) -> Vec<String> {
    w.ok("ListAliases", &json!({}))["Aliases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["AliasName"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn a_reset_leaves_kms_as_a_fresh_start_and_the_file_smaller() {
    let dir = temp();
    let clock = Arc::new(ManualClock::at(START));
    let mut w = World::with(Core::open_in(dir.path(), clock.clone()).unwrap(), clock);
    for n in 0..300 {
        let id = w.key(&json!({"Description": "d".repeat(200)}));
        w.ok(
            "CreateAlias",
            &json!({"AliasName": format!("alias/k{n}"), "TargetKeyId": id}),
        );
    }
    ssm_encrypts(&mut w);
    assert!(aliases(&mut w).contains(&"alias/aws/ssm".to_owned()));

    let DataReply::Usage(file) = w.core.data("us-east-1", DataAsk::Usage) else {
        panic!()
    };
    assert!(file.bytes > 300 * 300, "{file:?}");
    let DataReply::Emptied(Ok(dry)) = w.core.data("us-east-1", DataAsk::Reset { dry_run: true })
    else {
        panic!()
    };
    assert_eq!((dry.items, dry.freed), (602, 0));

    let DataReply::Emptied(Ok(done)) = w.core.data("us-east-1", DataAsk::Reset { dry_run: false })
    else {
        panic!()
    };
    assert!(done.freed > 0, "the file shrank: {done:?}");
    reopen(&mut w, dir.path());
    assert!(
        w.ok("ListKeys", &json!({}))["Keys"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(aliases(&mut w).is_empty());
    // The managed key comes back when SSM asks, and a new key seals and opens with the new sealing key.
    ssm_encrypts(&mut w);
    let id = w.key(&json!({}));
    let sealed = w.ok("Encrypt", &json!({"KeyId": id, "Plaintext": "aGk="}));
    reopen(&mut w, dir.path());
    let opened = w.ok(
        "Decrypt",
        &json!({"CiphertextBlob": sealed["CiphertextBlob"]}),
    );
    assert_eq!(opened["Plaintext"], "aGk=");
}
