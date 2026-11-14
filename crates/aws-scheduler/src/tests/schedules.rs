use serde_json::json;

use super::{ROLE, START, World, schedule};

const QUEUE: &str = "arn:aws:sqs:us-east-1:000000000000:q";

#[test]
fn a_schedule_reads_back_with_aws_defaults() {
    let mut w = World::new();
    let made = w.ok(
        "CreateSchedule",
        &schedule("nightly", "cron(0 2 * * ? *)", QUEUE),
    );
    assert_eq!(
        made["ScheduleArn"],
        "arn:aws:scheduler:us-east-1:000000000000:schedule/default/nightly"
    );
    let s = w.ok("GetSchedule", &json!({"Name": "nightly"}));
    assert_eq!(s["GroupName"], "default");
    assert_eq!(s["State"], "ENABLED");
    assert_eq!(s["ScheduleExpressionTimezone"], "UTC");
    assert_eq!(s["ActionAfterCompletion"], "NONE");
    assert_eq!(s["FlexibleTimeWindow"], json!({"Mode": "OFF"}));
    assert_eq!(s["Target"]["RoleArn"], ROLE);
    assert_eq!(
        s["Target"]["RetryPolicy"],
        json!({"MaximumEventAgeInSeconds": 86400, "MaximumRetryAttempts": 185})
    );
    assert_eq!(s["CreationDate"].as_f64(), Some((START / 1000) as f64));
    let (code, message) = w.err(
        "CreateSchedule",
        &schedule("nightly", "rate(1 hour)", QUEUE),
    );
    assert_eq!(
        (code.as_str(), message.as_str()),
        ("ConflictException", "Schedule nightly already exists.")
    );
}

#[test]
fn update_replaces_the_whole_schedule() {
    let mut w = World::new();
    let mut s = schedule("job", "rate(5 minutes)", QUEUE);
    s["Description"] = json!("first");
    s["ScheduleExpressionTimezone"] = json!("Europe/Paris");
    w.ok("CreateSchedule", &s);
    w.clock.advance(60_000);
    let mut u = schedule("job", "rate(10 minutes)", QUEUE);
    u["State"] = json!("DISABLED");
    w.ok("UpdateSchedule", &u);
    let got = w.ok("GetSchedule", &json!({"Name": "job"}));
    assert_eq!(got["ScheduleExpression"], "rate(10 minutes)");
    assert_eq!(got["State"], "DISABLED");
    assert!(
        got.get("Description").is_none(),
        "a member left out goes back to its default"
    );
    assert_eq!(got["ScheduleExpressionTimezone"], "UTC");
    assert_eq!(got["CreationDate"].as_f64(), Some((START / 1000) as f64));
    assert_eq!(
        got["LastModificationDate"].as_f64(),
        Some(((START + 60_000) / 1000) as f64)
    );
    let (code, _) = w.err("UpdateSchedule", &schedule("nope", "rate(1 hour)", QUEUE));
    assert_eq!(code, "ResourceNotFoundException");
}

#[test]
fn schedules_are_listed_by_group_prefix_and_state_in_pages() {
    let mut w = World::new();
    w.ok("CreateScheduleGroup", &json!({"Name": "g"}));
    for name in ["a1", "a2", "b1"] {
        w.ok("CreateSchedule", &schedule(name, "rate(1 hour)", QUEUE));
    }
    let mut off = schedule("a3", "rate(1 hour)", QUEUE);
    off["State"] = json!("DISABLED");
    off["GroupName"] = json!("g");
    w.ok("CreateSchedule", &off);
    let names = |v: &serde_json::Value| -> Vec<String> {
        v["Schedules"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["Name"].as_str().unwrap().to_owned())
            .collect()
    };
    assert_eq!(
        names(&w.ok("ListSchedules", &json!({}))),
        ["a1", "a2", "b1", "a3"]
    );
    assert_eq!(
        names(&w.ok("ListSchedules", &json!({"NamePrefix": "a"}))),
        ["a1", "a2", "a3"]
    );
    assert_eq!(
        names(&w.ok("ListSchedules", &json!({"State": "DISABLED"}))),
        ["a3"]
    );
    assert_eq!(
        names(&w.ok("ListSchedules", &json!({"GroupName": "g"}))),
        ["a3"]
    );
    let page = w.ok("ListSchedules", &json!({"MaxResults": 3}));
    assert_eq!(names(&page), ["a1", "a2", "b1"]);
    assert_eq!(page["Schedules"][0]["Target"]["Arn"], QUEUE);
    let rest = w.ok(
        "ListSchedules",
        &json!({"MaxResults": 3, "NextToken": page["NextToken"]}),
    );
    assert_eq!(names(&rest), ["a3"]);
    let (code, _) = w.err("ListSchedules", &json!({"NextToken": "!!"}));
    assert_eq!(code, "ValidationException");
}

#[test]
fn what_aws_refuses_is_refused() {
    let mut w = World::new();
    let refused = |w: &mut World, req: serde_json::Value| w.err("CreateSchedule", &req).0;
    assert_eq!(
        refused(&mut w, schedule("x", "rate(5 minute)", QUEUE)),
        "ValidationException"
    );
    assert_eq!(
        refused(&mut w, schedule("x", "at(2026-13-01T00:00:00)", QUEUE)),
        "ValidationException"
    );
    let mut tz = schedule("x", "cron(0 9 * * ? *)", QUEUE);
    tz["ScheduleExpressionTimezone"] = json!("Mars/Olympus");
    assert_eq!(refused(&mut w, tz), "ValidationException");
    let mut past = schedule("x", "rate(5 minutes)", QUEUE);
    past["EndDate"] = json!(START / 1000 - 60);
    assert_eq!(refused(&mut w, past), "ValidationException");
    let mut order = schedule("x", "rate(5 minutes)", QUEUE);
    order["StartDate"] = json!(START / 1000 + 7200);
    order["EndDate"] = json!(START / 1000 + 3600);
    assert_eq!(refused(&mut w, order), "ValidationException");
    let mut flex = schedule("x", "rate(5 minutes)", QUEUE);
    flex["FlexibleTimeWindow"] = json!({"Mode": "FLEXIBLE"});
    assert_eq!(refused(&mut w, flex), "ValidationException");
    let mut off = schedule("x", "rate(5 minutes)", QUEUE);
    off["FlexibleTimeWindow"] = json!({"Mode": "OFF", "MaximumWindowInMinutes": 5});
    assert_eq!(refused(&mut w, off), "ValidationException");
    let read_only = schedule(
        "x",
        "rate(5 minutes)",
        "arn:aws:scheduler:::aws-sdk:sqs:getQueueUrl",
    );
    assert_eq!(refused(&mut w, read_only), "ValidationException");
    assert_eq!(
        refused(&mut w, schedule("x", "rate(5 minutes)", "not-an-arn")),
        "ValidationException"
    );
    let mut nogroup = schedule("x", "rate(5 minutes)", QUEUE);
    nogroup["GroupName"] = json!("missing");
    assert_eq!(refused(&mut w, nogroup), "ResourceNotFoundException");
    // A one-time schedule ignores StartDate and EndDate, so a past EndDate is no fault.
    let mut once = schedule("x", "at(2026-06-01T12:00:00)", QUEUE);
    once["EndDate"] = json!(START / 1000 - 60);
    w.ok("CreateSchedule", &once);
}

#[test]
fn deleting_a_schedule_twice_says_it_is_gone() {
    let mut w = World::new();
    w.ok("CreateSchedule", &schedule("x", "rate(5 minutes)", QUEUE));
    w.ok("DeleteSchedule", &json!({"Name": "x"}));
    let (code, message) = w.err("DeleteSchedule", &json!({"Name": "x"}));
    assert_eq!(
        (code.as_str(), message.as_str()),
        ("ResourceNotFoundException", "Schedule x does not exist.")
    );
}

#[test]
fn creating_a_schedule_asks_for_pass_role() {
    let w = World::new();
    let h = aws_testkit::Harness::new(crate::Scheduler::new(super::ctx(
        &std::env::temp_dir(),
        None,
    )));
    let call = h
        .call(
            h.op("CreateSchedule"),
            &schedule("x", "rate(5 minutes)", QUEUE),
        )
        .unwrap();
    let needs: Vec<(String, String)> = w
        .core
        .needs(&call)
        .into_iter()
        .map(|n| (n.action, n.resource))
        .collect();
    assert_eq!(
        needs,
        [
            (
                "scheduler:CreateSchedule".to_owned(),
                "arn:aws:scheduler:us-east-1:000000000000:schedule/default/x".to_owned()
            ),
            ("iam:PassRole".to_owned(), ROLE.to_owned()),
        ]
    );
}
