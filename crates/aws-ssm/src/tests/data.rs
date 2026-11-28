//! `doze-aws data` on Parameter Store: a parameter with history is listed with a note, `--old-versions` removes the old versions no
//! label points at and keeps the value, other prunes are refused, and a reset removes every parameter.

use aws_wire::storage::{Emptied, Prune, StorageError};
use serde_json::json;

use super::{Bus, World, temp};
use crate::core::Core;
use crate::storage::{DataAsk, DataReply};

fn on_disk(dir: &std::path::Path) -> World {
    let clock = std::sync::Arc::new(doze_runtime::ManualClock::at(1_700_000_000_000));
    let bus = Bus::new();
    let core = Core::open_in(dir, clock.clone(), Some(bus.peers.clone())).unwrap();
    World::with(core, clock, bus)
}

fn reopen(w: World, dir: &std::path::Path) -> World {
    let mut w = w;
    w.core.db.take().unwrap().close();
    on_disk(dir)
}

fn ask(w: &mut World, ask: DataAsk) -> Result<Emptied, StorageError> {
    match data_now(&mut w.core, "us-east-1", ask) {
        DataReply::Emptied(done) => done,
        other => panic!("{other:?}"),
    }
}

fn old_versions(dry_run: bool) -> Prune {
    Prune {
        old_versions: true,
        dry_run,
        ..Prune::default()
    }
}

#[test]
fn a_parameter_with_history_is_listed_and_its_old_versions_removed() {
    let dir = temp();
    let mut w = on_disk(dir.path());
    let big = "v".repeat(4000);
    for n in 0..60 {
        w.ok(
            "PutParameter",
            &json!({"Name": "/app/config", "Value": format!("{n}{big}"), "Type": "String", "Overwrite": true}),
        );
    }
    w.ok(
        "LabelParameterVersion",
        &json!({"Name": "/app/config", "ParameterVersion": 3, "Labels": ["golden"]}),
    );
    w.ok(
        "PutParameter",
        &json!({"Name": "/app/once", "Value": "x", "Type": "String"}),
    );

    let DataReply::Usage(resources, _) = data_now(&mut w.core, "us-east-1", DataAsk::Usage) else {
        panic!()
    };
    assert_eq!(resources.len(), 1, "only a parameter with history");
    assert_eq!(resources[0].items, 60);
    assert!(resources[0].bytes > 60 * 4000);
    assert_eq!(
        resources[0].notes[0].text,
        "in 58 old versions no label points at"
    );

    let would = ask(
        &mut w,
        DataAsk::Empty {
            name: "/app/config".into(),
            prune: old_versions(true),
        },
    )
    .unwrap();
    assert_eq!((would.items, would.freed), (58, 0));
    let done = ask(
        &mut w,
        DataAsk::Empty {
            name: "/app/config".into(),
            prune: old_versions(false),
        },
    )
    .unwrap();
    assert_eq!(done.items, 58);
    assert!(done.freed > 0, "the file shrank: {done:?}");

    // The value, the labelled version and the version numbers stay, after a restart too.
    let mut w = reopen(w, dir.path());
    let got = w.ok("GetParameter", &json!({"Name": "/app/config"}));
    assert_eq!(got["Parameter"]["Version"], 60);
    let golden = w.ok("GetParameter", &json!({"Name": "/app/config:golden"}));
    assert_eq!(golden["Parameter"]["Version"], 3);
    let history = w.ok("GetParameterHistory", &json!({"Name": "/app/config"}));
    assert_eq!(history["Parameters"].as_array().unwrap().len(), 2);
}

#[test]
fn other_prunes_and_unknown_parameters_are_refused() {
    let dir = temp();
    let mut w = on_disk(dir.path());
    w.ok(
        "PutParameter",
        &json!({"Name": "/app/config", "Value": "x", "Type": "String"}),
    );
    for prune in [
        Prune::default(),
        Prune {
            before_ms: Some(1),
            old_versions: true,
            ..Prune::default()
        },
    ] {
        assert!(matches!(
            ask(
                &mut w,
                DataAsk::Empty {
                    name: "/app/config".into(),
                    prune
                }
            ),
            Err(StorageError::Unsupported(_))
        ));
    }
    assert_eq!(
        ask(
            &mut w,
            DataAsk::Empty {
                name: "/nope".into(),
                prune: old_versions(false)
            }
        ),
        Err(StorageError::NotFound)
    );
}

#[test]
fn a_reset_removes_every_parameter() {
    let dir = temp();
    let mut w = on_disk(dir.path());
    w.ok(
        "PutParameter",
        &json!({"Name": "a", "Value": "x", "Type": "String"}),
    );
    let done = ask(&mut w, DataAsk::Reset { dry_run: false }).unwrap();
    assert_eq!(done.items, 1);
    let mut w = reopen(w, dir.path());
    let all = w.ok("DescribeParameters", &json!({}));
    assert!(all["Parameters"].as_array().unwrap().is_empty());
    w.ok(
        "PutParameter",
        &json!({"Name": "a", "Value": "x", "Type": "String"}),
    );
}

/// A data question, on a runtime of one thread as the server's (a compaction goes to its blocking pool).
fn data_now(
    core: &mut crate::core::Core,
    region: &str,
    ask: crate::storage::DataAsk,
) -> crate::storage::DataReply {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(core.data(region, ask))
}
