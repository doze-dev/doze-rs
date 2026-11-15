//! `doze-aws data` on CloudWatch: a namespace's size is its data points, emptying keeps the alarms and dashboards, an older-than
//! prune keeps the newer data, alarm history is its own resource, and the file shrinks.

use std::sync::Arc;

use aws_wire::storage::{Emptied, Prune, StorageError};
use doze_runtime::ManualClock;
use serde_json::json;

use super::{NOW, World, temp};
use crate::core::Core;
use crate::storage::{ALARM_HISTORY, DataAsk, DataReply};

fn on_disk(dir: &std::path::Path) -> World {
    let clock = Arc::new(ManualClock::at(NOW));
    World::with(Core::open_in(dir, clock.clone(), None).unwrap(), clock)
}

/// `minutes` minutes of 150 values each for metric `m` of `namespace`, the newest a minute before now.
fn fill(w: &mut World, namespace: &str, minutes: i64) {
    let values: Vec<f64> = (0..150).map(f64::from).collect();
    for chunk in (1..=minutes).collect::<Vec<_>>().chunks(500) {
        let data: Vec<_> = chunk
            .iter()
            .map(|i| json!({"MetricName": "m", "Timestamp": (NOW - i * 60_000) as f64 / 1000.0, "Values": values}))
            .collect();
        w.ok(
            "PutMetricData",
            &json!({"Namespace": namespace, "MetricData": data}),
        );
    }
}

fn ask(w: &mut World, ask: DataAsk) -> Result<Emptied, StorageError> {
    match w.core.data("us-east-1", ask) {
        DataReply::Emptied(done) => done,
        other => panic!("{other:?}"),
    }
}

fn empty(name: &str, prune: Prune) -> DataAsk {
    DataAsk::Empty {
        name: name.into(),
        prune,
    }
}

#[test]
fn a_namespace_is_measured_emptied_and_its_space_given_back() {
    let dir = temp();
    let mut w = on_disk(dir.path());
    fill(&mut w, "App", 600);
    fill(&mut w, "Other", 2);
    w.alarm("high", &json!({"Namespace": "App"}));
    w.ok(
        "PutDashboard",
        &json!({"DashboardName": "d", "DashboardBody": "{\"widgets\":[]}"}),
    );

    let DataReply::Usage(resources, file) = w.core.data("us-east-1", DataAsk::Usage) else {
        panic!()
    };
    let of = |name: &str| resources.iter().find(|r| r.name == name).unwrap().clone();
    assert_eq!(of("App").items, 600);
    assert!(of("App").bytes > 600 * 150 * 8, "{:?}", of("App"));
    assert_eq!(of("Other").items, 2);
    assert_eq!(of(ALARM_HISTORY).items, 1, "the alarm's creation");
    assert!(file.bytes >= of("App").bytes);

    let dry = Prune {
        dry_run: true,
        ..Prune::default()
    };
    let would = ask(&mut w, empty("App", dry)).unwrap();
    assert_eq!((would.items, would.freed), (600, 0));

    // Older than five minutes ago: the newest five minutes stay.
    let older = Prune {
        before_ms: Some(NOW - 300_000),
        ..Prune::default()
    };
    let done = ask(&mut w, empty("App", older)).unwrap();
    assert_eq!(done.items, 595);
    assert!(done.freed > 0, "the file shrank: {done:?}");
    let listed = w.ok("ListMetrics", &json!({"Namespace": "App"}));
    assert_eq!(listed["Metrics"].as_array().unwrap().len(), 1);

    // Whole: the metric goes with its data; the alarm and the dashboard stay.
    let done = ask(&mut w, empty("App", Prune::default())).unwrap();
    assert_eq!(done.items, 5);
    let listed = w.ok("ListMetrics", &json!({"Namespace": "App"}));
    assert!(listed["Metrics"].as_array().unwrap().is_empty());
    assert_eq!(
        w.ok("DescribeAlarms", &json!({}))["MetricAlarms"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    w.ok("GetDashboard", &json!({"DashboardName": "d"}));

    // Alarm history goes on its own, and the alarm stays.
    assert_eq!(
        ask(&mut w, empty(ALARM_HISTORY, Prune::default()))
            .unwrap()
            .items,
        1
    );
    let h = w.ok("DescribeAlarmHistory", &json!({"AlarmName": "high"}));
    assert!(h["AlarmHistoryItems"].as_array().unwrap().is_empty());

    // What is left survives a restart.
    let old = std::mem::replace(&mut w.core, Core::in_memory(w.clock.clone(), None));
    old.db.unwrap().close();
    let mut w = on_disk(dir.path());
    let listed = w.ok("ListMetrics", &json!({}));
    assert_eq!(listed["Metrics"].as_array().unwrap().len(), 1, "Other");

    let prefix = Prune {
        prefix: Some("a".into()),
        ..Prune::default()
    };
    assert!(matches!(
        ask(&mut w, empty("Other", prefix)),
        Err(StorageError::Unsupported(_))
    ));
    assert_eq!(
        ask(&mut w, empty("App", Prune::default())),
        Err(StorageError::NotFound)
    );
}

#[test]
fn a_reset_removes_everything_and_survives_a_restart() {
    let dir = temp();
    let mut w = on_disk(dir.path());
    fill(&mut w, "App", 30);
    w.alarm("high", &json!({}));
    let done = ask(&mut w, DataAsk::Reset { dry_run: false }).unwrap();
    assert_eq!(done.items, 31);
    let old = std::mem::replace(&mut w.core, Core::in_memory(w.clock.clone(), None));
    old.db.unwrap().close();
    let mut w = on_disk(dir.path());
    assert!(
        w.ok("ListMetrics", &json!({}))["Metrics"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        w.ok("DescribeAlarms", &json!({}))["MetricAlarms"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    w.alarm("high", &json!({}));
}
