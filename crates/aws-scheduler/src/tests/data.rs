//! `doze-aws data` on Scheduler: nothing grows, so emptying is refused; a reset removes every group and schedule, the file shrinks,
//! and the `default` group is back on the next call, as on a fresh start.

use aws_testkit::TempDir;
use serde_json::json;

use super::{World, schedule};
use crate::core::Core;
use crate::storage::{DataAsk, DataReply};

fn saved(w: &mut World) {
    if let Some(ack) = w.core.flush().unwrap() {
        ack.wait().unwrap();
    }
}

#[test]
fn a_reset_removes_every_schedule_and_the_default_group_comes_back() {
    let dir = TempDir::new("scheduler-data");
    let mut w = World::new();
    w.core = Core::open_in(dir.path(), w.clock.clone(), Some(w.peers.clone())).unwrap();
    w.ok("CreateScheduleGroup", &json!({"Name": "jobs"}));
    let arn = "arn:aws:sqs:us-east-1:000000000000:q";
    for n in 0..300 {
        let mut s = schedule(&format!("s{n}"), "rate(1 hour)", arn);
        s["GroupName"] = json!("jobs");
        s["Description"] = json!("d".repeat(200));
        w.ok("CreateSchedule", &s);
    }
    saved(&mut w);
    let DataReply::Usage(file) = data_now(&mut w.core, "us-east-1", DataAsk::Usage) else {
        panic!()
    };
    assert!(file.bytes > 300 * 200, "{file:?}");
    let DataReply::Emptied(Ok(done)) =
        data_now(&mut w.core, "us-east-1", DataAsk::Reset { dry_run: false })
    else {
        panic!()
    };
    assert_eq!(done.items, 302, "300 schedules, `jobs` and `default`");
    assert!(done.freed > 0, "the file shrank: {done:?}");
    w.core.db.take().unwrap().close();
    w.core = Core::open_in(dir.path(), w.clock.clone(), Some(w.peers.clone())).unwrap();
    let groups = w.ok("ListScheduleGroups", &json!({}));
    let names: Vec<&str> = groups["ScheduleGroups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["Name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["default"]);
    assert!(
        w.ok("ListSchedules", &json!({}))["Schedules"]
            .as_array()
            .unwrap()
            .is_empty()
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
