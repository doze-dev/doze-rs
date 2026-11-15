//! `doze-aws data` on Step Functions: a machine's size is its executions and their history, emptying removes ended executions and
//! keeps the machine and the running ones, an older-than prune keeps what ended later, and the file shrinks.

use aws_wire::storage::{Prune, Storage, StorageError};
use serde_json::{Value, json};

use super::World;

fn pass(n: usize) -> Value {
    // Ten Pass states with a fat result: a history worth measuring.
    let mut states = serde_json::Map::new();
    for i in 0..n {
        let next = if i + 1 == n {
            json!({"Type": "Pass", "Result": "x".repeat(4 << 10), "End": true})
        } else {
            json!({"Type": "Pass", "Result": "x".repeat(4 << 10), "Next": format!("P{}", i + 1)})
        };
        states.insert(format!("P{i}"), next);
    }
    json!({"StartAt": "P0", "States": states})
}

#[tokio::test]
async fn a_machine_is_measured_emptied_of_ended_executions_and_its_space_given_back() {
    let w = World::new();
    let m = w.machine("busy", pass(10)).await;
    let mut ended = Vec::new();
    for _ in 0..30 {
        ended.push(w.run(&m, json!({})).await);
    }
    let waits = w
        .machine(
            "waits",
            json!({"StartAt": "W", "States": {"W": {"Type": "Wait", "Seconds": 600, "End": true}}}),
        )
        .await;
    let running = w.start(&waits, json!({})).await;

    let usage = w.sfn.usage().await.unwrap();
    let of = |name: &str| {
        usage
            .resources
            .iter()
            .find(|r| r.name == name)
            .unwrap()
            .clone()
    };
    assert_eq!(of("busy").items, 30);
    assert!(of("busy").bytes > 30 * 10 * (4 << 10), "{:?}", of("busy"));
    assert_eq!(of("busy").notes.len(), 1, "ended executions dominate");
    assert!(of("waits").notes[0].text.contains("running"));
    assert_eq!(usage.files.len(), 1);

    // A dry run counts and removes nothing.
    let dry = Prune {
        dry_run: true,
        ..Prune::default()
    };
    let would = w.sfn.empty("us-east-1", "busy", &dry).await.unwrap();
    assert_eq!((would.items, would.freed), (30, 0));

    // Running executions are never removed; the machine stays.
    let none = w
        .sfn
        .empty("us-east-1", "waits", &Prune::default())
        .await
        .unwrap();
    assert_eq!(none.items, 0);
    let still = w
        .ok("DescribeExecution", json!({"executionArn": running}))
        .await;
    assert_eq!(still["status"], "RUNNING");

    let done = w
        .sfn
        .empty("us-east-1", "busy", &Prune::default())
        .await
        .unwrap();
    assert_eq!(done.items, 30);
    assert!(done.freed > 1 << 20, "the file shrank: {done:?}");
    let gone = w
        .call(
            "DescribeExecution",
            json!({"executionArn": ended[0]["executionArn"]}),
        )
        .await;
    assert!(gone.is_err());
    w.ok("DescribeStateMachine", json!({"stateMachineArn": m}))
        .await;
    assert_eq!(w.run(&m, json!({})).await["status"], "SUCCEEDED");

    // Survives a restart: one execution left.
    let dir = w.stop().await;
    let w = World::at(dir);
    let listed = w.ok("ListExecutions", json!({"stateMachineArn": m})).await;
    assert_eq!(listed["executions"].as_array().unwrap().len(), 1);
    w.stop().await;
}

#[tokio::test]
async fn other_prunes_and_unknown_machines_are_refused() {
    let w = World::new();
    let m = w.machine("busy", pass(1)).await;
    w.run(&m, json!({})).await;
    let prefix = Prune {
        prefix: Some("a".into()),
        ..Prune::default()
    };
    assert!(matches!(
        w.sfn.empty("us-east-1", "busy", &prefix).await,
        Err(StorageError::Unsupported(_))
    ));
    assert_eq!(
        w.sfn.empty("us-east-1", "nope", &Prune::default()).await,
        Err(StorageError::NotFound)
    );
    w.stop().await;
}

#[tokio::test]
async fn an_older_than_prune_keeps_what_ended_later() {
    let w = World::new();
    let m = w.machine("m", pass(2)).await;
    w.run(&m, json!({})).await;
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    let cut = doze_runtime::Clock::now_ms(&doze_runtime::SystemClock);
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    let later = w.run(&m, json!({})).await;
    let older = Prune {
        before_ms: Some(cut),
        ..Prune::default()
    };
    let done = w.sfn.empty("us-east-1", "m", &older).await.unwrap();
    assert_eq!(done.items, 1);
    let listed = w.ok("ListExecutions", json!({"stateMachineArn": m})).await;
    assert_eq!(
        listed["executions"][0]["executionArn"],
        later["executionArn"]
    );
    // Its history is still whole.
    assert!(
        !w.history(later["executionArn"].as_str().unwrap())
            .await
            .is_empty()
    );
    w.stop().await;
}

#[tokio::test]
async fn a_reset_removes_everything_and_survives_a_restart() {
    let w = World::new();
    let m = w.machine("m", pass(2)).await;
    w.run(&m, json!({})).await;
    w.ok("CreateActivity", json!({"name": "act"})).await;
    let done = w.sfn.reset(false).await.unwrap();
    assert_eq!(done.items, 1);
    let dir = w.stop().await;
    let w = World::at(dir);
    let machines = w.ok("ListStateMachines", json!({})).await;
    assert!(machines["stateMachines"].as_array().unwrap().is_empty());
    let activities = w.ok("ListActivities", json!({})).await;
    assert!(activities["activities"].as_array().unwrap().is_empty());
    // The name is free at once.
    w.machine("m", pass(2)).await;
    w.stop().await;
}
