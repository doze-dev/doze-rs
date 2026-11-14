use serde_json::json;

use super::{START, World, schedule};
use crate::model::key;

const QUEUE: &str = "arn:aws:sqs:us-east-1:000000000000:q";
const MINUTE: i64 = 60_000;

fn next_ms(w: &World, name: &str) -> Option<i64> {
    w.core
        .schedules
        .get(&key(super::ACCOUNT, &["default", name]))
        .and_then(|s| s.next_ms)
}

#[test]
fn a_rate_without_a_start_date_fires_at_once_then_every_period() {
    let mut w = World::new();
    w.ok("CreateSchedule", &schedule("r", "rate(5 minutes)", QUEUE));
    assert_eq!(next_ms(&w, "r"), Some(START));
    assert_eq!(w.core.next_due(), Some(START));
    let fired = w.tick();
    assert_eq!(fired.len(), 1);
    assert_eq!(fired[0].scheduled_ms, START);
    assert_eq!(fired[0].execution_id.len(), 16);
    assert_eq!(next_ms(&w, "r"), Some(START + 5 * MINUTE));
    assert!(w.tick().is_empty(), "nothing is due before its time");
    // A long pause does not replay what it missed: one fire, then the next slot after now.
    w.clock.advance(23 * MINUTE);
    assert_eq!(w.tick().len(), 1);
    assert_eq!(next_ms(&w, "r"), Some(START + 25 * MINUTE));
}

#[test]
fn a_rate_with_a_start_date_counts_from_it_and_stops_before_the_end() {
    let mut w = World::new();
    let mut s = schedule("r", "rate(1 hour)", QUEUE);
    s["StartDate"] = json!((START + 30 * MINUTE) / 1000);
    s["EndDate"] = json!((START + 90 * MINUTE) / 1000);
    s["ActionAfterCompletion"] = json!("DELETE");
    w.ok("CreateSchedule", &s);
    assert_eq!(next_ms(&w, "r"), Some(START + 30 * MINUTE));
    w.clock.set(START + 30 * MINUTE);
    assert_eq!(w.tick().len(), 1);
    // The next slot (START + 90 minutes) is not before the end: the schedule is complete and deletes itself.
    assert!(w.core.schedules.is_empty());
}

#[test]
fn a_cron_schedule_fires_on_its_zone_clock() {
    let mut w = World::new();
    let mut s = schedule("c", "cron(30 8 * * ? *)", QUEUE);
    s["ScheduleExpressionTimezone"] = json!("America/New_York");
    w.ok("CreateSchedule", &s);
    // 08:30 EST on 2026-01-01 is 13:30 UTC.
    assert_eq!(next_ms(&w, "c"), Some(START + (13 * 60 + 30) * MINUTE));
}

#[test]
fn a_one_time_schedule_fires_once_and_deletes_itself_when_asked() {
    let mut w = World::new();
    let mut s = schedule("once", "at(2026-01-01T00:01:00)", QUEUE);
    s["ActionAfterCompletion"] = json!("DELETE");
    w.ok("CreateSchedule", &s);
    let mut keep = schedule("kept", "at(2026-01-01T00:01:00)", QUEUE);
    keep["ActionAfterCompletion"] = json!("NONE");
    w.ok("CreateSchedule", &keep);
    assert_eq!(w.core.next_due(), Some(START + MINUTE));
    w.clock.advance(MINUTE);
    assert_eq!(w.tick().len(), 2);
    let (code, _) = w.err("GetSchedule", &json!({"Name": "once"}));
    assert_eq!(code, "ResourceNotFoundException");
    w.ok("GetSchedule", &json!({"Name": "kept"}));
    assert_eq!(next_ms(&w, "kept"), None);
    assert_eq!(
        w.core.next_due(),
        None,
        "nothing left to wait for: no timer"
    );
}

#[test]
fn a_disabled_schedule_has_no_next_fire() {
    let mut w = World::new();
    let mut s = schedule("d", "rate(1 minute)", QUEUE);
    s["State"] = json!("DISABLED");
    w.ok("CreateSchedule", &s);
    assert_eq!(w.core.next_due(), None);
    s["State"] = json!("ENABLED");
    w.ok("UpdateSchedule", &s);
    assert_eq!(w.core.next_due(), Some(START));
}

#[test]
fn after_a_restart_a_missed_one_time_schedule_fires_and_a_recurring_one_is_replanned() {
    let mut w = World::new();
    w.ok(
        "CreateSchedule",
        &schedule("once", "at(2026-01-01T00:05:00)", QUEUE),
    );
    w.ok(
        "CreateSchedule",
        &schedule("every", "cron(*/10 * * * ? *)", QUEUE),
    );
    w.clock.advance(60 * MINUTE + 1000);
    let answer = w.rt.block_on(doze_runtime::Service::handle(
        &mut w.core,
        crate::core::Request::Resume,
    ));
    let fired: Vec<_> = answer
        .fired
        .iter()
        .map(|i| i.schedule_arn.rsplit('/').next().unwrap().to_owned())
        .collect();
    assert_eq!(fired, ["once"]);
    assert_eq!(next_ms(&w, "every"), Some(START + 70 * MINUTE));
}

#[test]
fn the_hook_fires_now_and_a_one_time_fire_completes_it() {
    let mut w = World::new();
    let mut s = schedule("once", "at(2026-06-01T00:00:00)", QUEUE);
    s["ActionAfterCompletion"] = json!("DELETE");
    w.ok("CreateSchedule", &s);
    w.ok("CreateSchedule", &schedule("every", "rate(1 day)", QUEUE));
    w.core.fire_now(None, "default", "every").unwrap();
    assert_eq!(std::mem::take(&mut w.core.fired).len(), 1);
    assert_eq!(
        next_ms(&w, "every"),
        Some(START),
        "a recurring schedule keeps its plan"
    );
    w.core
        .fire_now(Some(super::ACCOUNT), "default", "once")
        .unwrap();
    assert_eq!(w.core.fired[0].scheduled_ms, START);
    assert!(
        w.core
            .schedules
            .get(&key(super::ACCOUNT, &["default", "once"]))
            .is_none()
    );
    let e = w.core.fire_now(None, "default", "nope").unwrap_err();
    assert_eq!(e.code, "ResourceNotFoundException");
}
