//! Composite and log alarms, evaluated; and which kinds `DescribeAlarms` shows.

use std::sync::Arc;
use std::time::Duration;

use aws_peers::Peers;
use aws_wire::service::Caller;
use doze_runtime::{Clock, Ctx, ManualClock, Tasks};
use serde_json::{Value, json};

use super::{NOW, World, temp};
use crate::Cloudwatch;
use crate::log_eval::LogOutcome;

fn log_alarm(name: &str) -> Value {
    json!({
        "AlarmName": name, "Threshold": 5.0, "ComparisonOperator": "GreaterThanThreshold",
        "QueryResultsToEvaluate": 3, "QueryResultsToAlarm": 2,
        "ScheduledQueryConfiguration": {
            "QueryString": "fields @message | filter @message like /ERROR/", "LogGroupIdentifiers": ["/app"],
            "ScheduledQueryRoleARN": "arn:aws:iam::000000000000:role/q", "AggregationExpression": "count(*)",
            "ScheduleConfiguration": {"ScheduleExpression": "rate(5 minutes)", "StartTimeOffset": 300}},
    })
}

fn composite(w: &mut World, name: &str, rule: &str) {
    w.ok(
        "PutCompositeAlarm",
        &json!({"AlarmName": name, "AlarmRule": rule}),
    );
}

fn composite_state(w: &mut World, name: &str) -> (String, String) {
    let d = w.ok(
        "DescribeAlarms",
        &json!({"AlarmNames": [name], "AlarmTypes": ["CompositeAlarm"]}),
    );
    let a = &d["CompositeAlarms"][0];
    (
        a["StateValue"].as_str().unwrap().to_owned(),
        a["StateReason"].as_str().unwrap().to_owned(),
    )
}

fn force(w: &mut World, name: &str, state: &str) {
    w.ok(
        "SetAlarmState",
        &json!({"AlarmName": name, "StateValue": state, "StateReason": "forced"}),
    );
}

fn log_state(w: &mut World, name: &str) -> Value {
    let d = w.ok(
        "DescribeAlarms",
        &json!({"AlarmNames": [name], "AlarmTypes": ["LogAlarm"]}),
    );
    d["LogAlarms"][0].clone()
}

#[test]
fn describe_alarms_shows_metric_alarms_only_unless_asked_for_other_kinds() {
    let mut w = World::new();
    w.alarm("m", &json!({}));
    composite(&mut w, "c", "ALARM(m)");
    w.ok("PutLogAlarm", &log_alarm("l"));
    let all = w.ok("DescribeAlarms", &json!({}));
    assert_eq!(all["MetricAlarms"].as_array().unwrap().len(), 1);
    assert_eq!(all["CompositeAlarms"], json!([]));
    assert_eq!(all["LogAlarms"], json!([]));
    let named = w.ok("DescribeAlarms", &json!({"AlarmNames": ["c", "l"]}));
    assert_eq!(
        named["CompositeAlarms"],
        json!([]),
        "named, still not asked for"
    );
    let both = w.ok(
        "DescribeAlarms",
        &json!({"AlarmTypes": ["CompositeAlarm", "LogAlarm"]}),
    );
    assert_eq!(both["MetricAlarms"], json!([]));
    assert_eq!(both["CompositeAlarms"].as_array().unwrap().len(), 1);
    assert_eq!(both["LogAlarms"].as_array().unwrap().len(), 1);
    // Recorded 2026-11-19: members not sent are not filled in.
    let l = &both["LogAlarms"][0];
    assert!(
        l.get("TreatMissingData").is_none() && l.get("ActionLogLineCount").is_none(),
        "{l}"
    );
    let mut more = log_alarm("l");
    more["QueryResultsToAlarm"] = json!(4);
    let (_, msg) = w.err("PutLogAlarm", &more);
    assert_eq!(
        msg,
        "QueryResultsToAlarm must be  smaller or equal than QueryResultsToEvaluate"
    );
    let metric = w.ok("DescribeAlarms", &json!({"AlarmTypes": ["MetricAlarm"]}));
    assert_eq!(metric["MetricAlarms"].as_array().unwrap().len(), 1);
}

#[test]
fn a_composite_follows_its_rule_as_its_children_change() {
    let mut w = World::new();
    w.alarm("cpu", &json!({}));
    w.alarm("disk", &json!({}));
    composite(&mut w, "both", "ALARM(cpu) AND ALARM(\"disk\")");
    // Evaluated as it is made: the rule does not hold, so OK, never INSUFFICIENT_DATA again.
    assert_eq!(composite_state(&mut w, "both").0, "OK");
    force(&mut w, "cpu", "ALARM");
    assert_eq!(composite_state(&mut w, "both").0, "OK");
    force(&mut w, "disk", "ALARM");
    let (state, reason) = composite_state(&mut w, "both");
    assert_eq!(state, "ALARM");
    assert!(
        reason
            .contains("arn:aws:cloudwatch:us-east-1:000000000000:alarm:disk transitioned to ALARM"),
        "{reason}"
    );
    // A composite of a composite follows it.
    composite(&mut w, "top", "ALARM(both) OR TRUE AND FALSE");
    assert_eq!(composite_state(&mut w, "top").0, "ALARM");
    force(&mut w, "cpu", "OK");
    assert_eq!(composite_state(&mut w, "both").0, "OK");
    assert_eq!(composite_state(&mut w, "top").0, "OK");
    let h = w.ok(
        "DescribeAlarmHistory",
        &json!({"AlarmName": "both", "HistoryItemType": "StateUpdate"}),
    );
    let summaries: Vec<&str> = h["AlarmHistoryItems"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["HistorySummary"].as_str().unwrap())
        .collect();
    assert_eq!(
        summaries,
        [
            "Alarm updated from ALARM to OK",
            "Alarm updated from OK to ALARM",
            "Alarm updated from INSUFFICIENT_DATA to OK",
        ]
    );
    // Deleting a child: the composite finds it in no state.
    force(&mut w, "cpu", "ALARM");
    composite(&mut w, "not-gone", "ALARM(disk)");
    assert_eq!(composite_state(&mut w, "not-gone").0, "ALARM");
    w.ok("DeleteAlarms", &json!({"AlarmNames": ["disk"]}));
    assert_eq!(composite_state(&mut w, "not-gone").0, "OK");
}

#[test]
fn a_cycle_of_composites_is_judged_once_per_change_and_ends() {
    let mut w = World::new();
    composite(&mut w, "x", "NOT ALARM(y)");
    assert_eq!(composite_state(&mut w, "x").0, "ALARM");
    composite(&mut w, "y", "ALARM(x)");
    let (x, y) = (
        composite_state(&mut w, "x").0,
        composite_state(&mut w, "y").0,
    );
    assert!(["ALARM", "OK"].contains(&x.as_str()) && ["ALARM", "OK"].contains(&y.as_str()));
}

#[test]
fn a_rule_that_does_not_read_is_refused() {
    let mut w = World::new();
    for rule in ["ALARM(cpu", "MAYBE(cpu)", "ALARM()", "ALARM(cpu) AND"] {
        let (code, _) = w.err(
            "PutCompositeAlarm",
            &json!({"AlarmName": "c", "AlarmRule": rule}),
        );
        assert_eq!(code, "ValidationException", "{rule}");
    }
    let (_, message) = w.err(
        "PutCompositeAlarm",
        &json!({"AlarmName": "c", "AlarmRule": "ALARM(c)"}),
    );
    assert!(message.contains("cannot name itself"), "{message}");
}

/// The next scheduled run, five minutes on, answered with `values`: the alarm as described after it.
fn run(w: &mut World, values: Vec<f64>) -> Value {
    w.clock.advance(300_000);
    w.core.tick();
    let r = w.core.log_runs.pop().expect("a run is due");
    assert!(w.core.log_runs.is_empty());
    assert_eq!(
        r.query,
        "fields @message | filter @message like /ERROR/ | stats count(*)"
    );
    assert_eq!(r.range_ms, (r.at_ms - 300_000, r.at_ms));
    assert_eq!(r.value_field, "count(*)");
    w.core.apply_log_outcome(LogOutcome {
        run: r,
        result: Ok(values),
    });
    log_state(w, "errors")
}

#[test]
fn a_log_alarm_runs_its_query_on_schedule_and_is_judged_m_out_of_n() {
    let mut w = World::new();
    w.ok("PutLogAlarm", &log_alarm("errors"));
    assert_eq!(
        log_state(&mut w, "errors")["StateReason"],
        "Unchecked: Initial alarm creation"
    );
    // The first run is a rate after the put.
    assert_eq!(w.core.earliest_due(), Some(NOW + 300_000));
    w.core.tick();
    assert!(w.core.log_runs.is_empty(), "nothing is due yet");
    let first = run(&mut w, vec![7.0]);
    // One of three breaching, two unknown: not enough to alarm, and not enough to be OK.
    assert_eq!(first["StateValue"], "INSUFFICIENT_DATA");
    let second = run(&mut w, vec![2.0, 9.0]);
    assert_eq!(second["StateValue"], "ALARM", "{second}");
    assert!(
        second["StateReason"]
            .as_str()
            .unwrap()
            .starts_with("Threshold Crossed: 2 out of the last 3")
    );
    run(&mut w, vec![1.0]);
    let quiet = run(&mut w, vec![]);
    // [9, 1, missing]: one breaching, so not ALARM; with the missing result unknown, not OK either.
    assert_eq!(quiet["StateValue"], "INSUFFICIENT_DATA");
    let ok = run(&mut w, vec![0.0]);
    assert_eq!(ok["StateValue"], "OK", "{ok}");
    assert_eq!(w.core.earliest_due(), Some(NOW + 6 * 300_000));
}

#[test]
fn a_failed_query_is_an_evaluation_error_and_an_answer_for_an_old_configuration_is_dropped() {
    let mut w = World::new();
    w.ok("PutLogAlarm", &log_alarm("errors"));
    w.clock.advance(300_000);
    w.core.tick();
    let r = w.core.log_runs.pop().unwrap();
    w.core.apply_log_outcome(LogOutcome {
        run: r.clone(),
        result: Err("ResourceNotFoundException: Log group '/app' does not exist".into()),
    });
    let a = log_state(&mut w, "errors");
    assert_eq!(a["EvaluationState"], "EVALUATION_ERROR");
    assert!(
        a["StateReason"]
            .as_str()
            .unwrap()
            .contains("does not exist")
    );
    // Put again: a new configuration, and the old run's answer means nothing to it.
    w.clock.advance(1000);
    w.ok("PutLogAlarm", &log_alarm("errors"));
    w.core.apply_log_outcome(LogOutcome {
        run: r,
        result: Ok(vec![100.0]),
    });
    assert_eq!(
        log_state(&mut w, "errors")["StateValue"],
        "INSUFFICIENT_DATA"
    );
    w.ok("DeleteAlarms", &json!({"AlarmNames": ["errors"]}));
    assert_eq!(w.core.earliest_due(), None);
}

/// CloudWatch and CloudWatch Logs on one bus, with a clock the test moves.
fn bus(dir: &std::path::Path, clock: Arc<ManualClock>) -> (Arc<Peers>, Tasks) {
    let peers = Peers::new();
    let tasks = Tasks::new();
    let ext: Arc<dyn std::any::Any + Send + Sync> = peers.clone();
    let ctx = |sub: &str| {
        Arc::new(Ctx {
            dir: dir.join(sub),
            idle: Duration::from_secs(300),
            tasks: tasks.clone(),
            clock: clock.clone(),
            ext: Some(Arc::clone(&ext)),
        })
    };
    peers.register(Arc::new(Cloudwatch::new(ctx("cloudwatch"))));
    peers.register(Arc::new(aws_logs::Logs::new(ctx("logs"))));
    (peers, tasks)
}

#[tokio::test]
async fn a_log_alarm_reads_the_logs_through_insights() {
    let dir = temp();
    let clock = Arc::new(ManualClock::at(1_700_000_000_000));
    let (peers, _tasks) = bus(dir.path(), clock.clone());
    let caller = Caller {
        access_key_id: "test".into(),
        account_id: "000000000000".into(),
        region: "us-east-1".into(),
        endpoint: String::new(),
        service: None,
        session_token: None,
    };
    let call = |service: &'static str, op: &'static str, body: Value| {
        let (peers, caller) = (Arc::clone(&peers), caller.clone());
        async move {
            peers
                .call("test", &caller, service, op, &body)
                .await
                .unwrap_or_else(|e| panic!("{op}: {}", e.message))
        }
    };
    call("logs", "CreateLogGroup", json!({"logGroupName": "/app"})).await;
    call(
        "logs",
        "CreateLogStream",
        json!({"logGroupName": "/app", "logStreamName": "s"}),
    )
    .await;
    let mut alarm = log_alarm("errors");
    alarm["QueryResultsToEvaluate"] = json!(1);
    alarm["QueryResultsToAlarm"] = json!(1);
    alarm["Threshold"] = json!(2.0);
    alarm["ScheduledQueryConfiguration"]["ScheduleConfiguration"]["ScheduleExpression"] =
        json!("rate(1 minute)");
    call("monitoring", "PutLogAlarm", alarm).await;
    let now = clock.now_ms();
    let events: Vec<Value> = ["ERROR a", "INFO b", "ERROR c", "ERROR d"]
        .iter()
        .map(|m| json!({"timestamp": now + 1000, "message": m}))
        .collect();
    call(
        "logs",
        "PutLogEvents",
        json!({"logGroupName": "/app", "logStreamName": "s", "logEvents": events}),
    )
    .await;
    clock.advance(61_000);
    // Any call tells the timer its deadline again; it has passed, so the query runs.
    let describe = json!({"AlarmNames": ["errors"], "AlarmTypes": ["LogAlarm"]});
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    loop {
        let d = call("monitoring", "DescribeAlarms", describe.clone()).await;
        let a = &d["LogAlarms"][0];
        if a["StateValue"] == "ALARM" {
            assert!(a["StateReason"].as_str().unwrap().contains("3.0"), "{a}");
            break;
        }
        assert!(tokio::time::Instant::now() < deadline, "never alarmed: {a}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
