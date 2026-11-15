//! `doze-aws data` on Secrets Manager: a secret with versions is listed with a note, `--old-versions` removes the versions with no
//! staging label and keeps the current and previous values, other prunes are refused, and a reset removes every secret.

use aws_wire::storage::{Emptied, Prune, StorageError};
use serde_json::json;

use super::{Bus, START, World, temp};
use crate::core::Core;
use crate::storage::{DataAsk, DataReply};

fn on_disk(dir: &std::path::Path) -> World {
    let clock = std::sync::Arc::new(doze_runtime::ManualClock::at(START));
    let bus = Bus::new();
    let core = Core::open_in(dir, clock.clone(), Some(bus.peers.clone())).unwrap();
    World::with(core, clock, bus)
}

/// The service stopped and started again over the same directory, with the same KMS beside it.
fn reopen(w: World, dir: &std::path::Path) -> World {
    let mut w = w;
    w.core.db.take().unwrap().close();
    w.core = Core::open_in(dir, w.clock.clone(), Some(w.bus.peers.clone())).unwrap();
    w
}

fn ask(w: &mut World, ask: DataAsk) -> Result<Emptied, StorageError> {
    match w.core.data("us-east-1", ask) {
        DataReply::Emptied(done) => done,
        other => panic!("{other:?}"),
    }
}

fn old_versions(name: &str, dry_run: bool) -> DataAsk {
    DataAsk::Empty {
        name: name.into(),
        prune: Prune {
            old_versions: true,
            dry_run,
            ..Prune::default()
        },
    }
}

#[test]
fn a_secret_with_versions_is_listed_and_its_unlabelled_versions_removed() {
    let dir = temp();
    let mut w = on_disk(dir.path());
    let big = "s".repeat(4000);
    w.ok(
        "CreateSecret",
        &json!({"Name": "db", "SecretString": format!("0{big}")}),
    );
    for n in 1..40 {
        w.ok(
            "PutSecretValue",
            &json!({"SecretId": "db", "SecretString": format!("{n}{big}")}),
        );
    }
    w.ok("CreateSecret", &json!({"Name": "one", "SecretString": "x"}));

    let DataReply::Usage(resources, _) = w.core.data("us-east-1", DataAsk::Usage) else {
        panic!()
    };
    assert_eq!(resources.len(), 1, "only a secret with versions");
    assert_eq!(resources[0].items, 40);
    assert_eq!(
        resources[0].notes[0].text,
        "in 38 versions with no staging label"
    );

    let would = ask(&mut w, old_versions("db", true)).unwrap();
    assert_eq!((would.items, would.freed), (38, 0));
    let done = ask(&mut w, old_versions("db", false)).unwrap();
    assert_eq!(done.items, 38);
    assert!(done.freed > 0, "the file shrank: {done:?}");

    let mut w = reopen(w, dir.path());
    let got = w.ok("GetSecretValue", &json!({"SecretId": "db"}));
    assert!(got["SecretString"].as_str().unwrap().starts_with("39"));
    let previous = w.ok(
        "GetSecretValue",
        &json!({"SecretId": "db", "VersionStage": "AWSPREVIOUS"}),
    );
    assert!(previous["SecretString"].as_str().unwrap().starts_with("38"));
    let listed = w.ok(
        "ListSecretVersionIds",
        &json!({"SecretId": "db", "IncludeDeprecated": true}),
    );
    assert_eq!(listed["Versions"].as_array().unwrap().len(), 2);

    assert!(matches!(
        ask(
            &mut w,
            DataAsk::Empty {
                name: "db".into(),
                prune: Prune::default()
            }
        ),
        Err(StorageError::Unsupported(_))
    ));
    assert_eq!(
        ask(&mut w, old_versions("nope", false)),
        Err(StorageError::NotFound)
    );
}

#[test]
fn a_reset_removes_every_secret_and_the_name_is_free() {
    let dir = temp();
    let mut w = on_disk(dir.path());
    w.ok("CreateSecret", &json!({"Name": "db", "SecretString": "x"}));
    w.ok(
        "DeleteSecret",
        &json!({"SecretId": "db", "RecoveryWindowInDays": 7}),
    );
    let done = ask(&mut w, DataAsk::Reset { dry_run: false }).unwrap();
    assert_eq!(done.items, 1);
    let mut w = reopen(w, dir.path());
    let all = w.ok("ListSecrets", &json!({"IncludePlannedDeletion": true}));
    assert!(all["SecretList"].as_array().unwrap().is_empty());
    // A secret scheduled for deletion holds its name on AWS; after a reset it is free at once.
    w.ok("CreateSecret", &json!({"Name": "db", "SecretString": "x"}));
}
