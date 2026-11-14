use serde_json::json;

use super::{World, schedule};

const QUEUE: &str = "arn:aws:sqs:us-east-1:000000000000:q";

#[test]
fn every_account_has_a_default_group_it_cannot_delete() {
    let mut w = World::new();
    let g = w.ok("GetScheduleGroup", &json!({"Name": "default"}));
    assert_eq!(
        g["Arn"],
        "arn:aws:scheduler:us-east-1:000000000000:schedule-group/default"
    );
    assert_eq!(g["State"], "ACTIVE");
    let (code, _) = w.err("DeleteScheduleGroup", &json!({"Name": "default"}));
    assert_eq!(code, "ValidationException");
}

#[test]
fn groups_are_made_listed_and_refused_twice() {
    let mut w = World::new();
    let made = w.ok(
        "CreateScheduleGroup",
        &json!({"Name": "jobs", "Tags": [{"Key": "team", "Value": "a"}]}),
    );
    assert_eq!(
        made["ScheduleGroupArn"],
        "arn:aws:scheduler:us-east-1:000000000000:schedule-group/jobs"
    );
    let (code, message) = w.err("CreateScheduleGroup", &json!({"Name": "jobs"}));
    assert_eq!(
        (code.as_str(), message.as_str()),
        ("ConflictException", "Schedule group jobs already exists.")
    );
    w.ok("CreateScheduleGroup", &json!({"Name": "other"}));
    let all = w.ok("ListScheduleGroups", &json!({}));
    let names: Vec<_> = all["ScheduleGroups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["Name"].clone())
        .collect();
    assert_eq!(names, [json!("default"), json!("jobs"), json!("other")]);
    let some = w.ok("ListScheduleGroups", &json!({"NamePrefix": "jo"}));
    assert_eq!(some["ScheduleGroups"].as_array().unwrap().len(), 1);
    let first = w.ok("ListScheduleGroups", &json!({"MaxResults": 2}));
    assert_eq!(first["ScheduleGroups"].as_array().unwrap().len(), 2);
    let next = w.ok(
        "ListScheduleGroups",
        &json!({"MaxResults": 2, "NextToken": first["NextToken"]}),
    );
    assert_eq!(next["ScheduleGroups"][0]["Name"], "other");
    assert!(next.get("NextToken").is_none());
    let (code, _) = w.err("GetScheduleGroup", &json!({"Name": "nope"}));
    assert_eq!(code, "ResourceNotFoundException");
}

#[test]
fn deleting_a_group_deletes_its_schedules() {
    let mut w = World::new();
    w.ok("CreateScheduleGroup", &json!({"Name": "jobs"}));
    let mut s = schedule("a", "rate(5 minutes)", QUEUE);
    s["GroupName"] = json!("jobs");
    w.ok("CreateSchedule", &s);
    w.ok("CreateSchedule", &schedule("b", "rate(5 minutes)", QUEUE));
    w.ok("DeleteScheduleGroup", &json!({"Name": "jobs"}));
    let (code, _) = w.err("GetSchedule", &json!({"Name": "a", "GroupName": "jobs"}));
    assert_eq!(code, "ResourceNotFoundException");
    w.ok("GetSchedule", &json!({"Name": "b"}));
    let (code, _) = w.err("ListSchedules", &json!({"GroupName": "jobs"}));
    assert_eq!(code, "ResourceNotFoundException");
}

#[test]
fn tags_live_on_groups() {
    let mut w = World::new();
    w.ok(
        "CreateScheduleGroup",
        &json!({"Name": "jobs", "Tags": [{"Key": "a", "Value": "1"}]}),
    );
    let arn = "arn:aws:scheduler:us-east-1:000000000000:schedule-group/jobs";
    w.ok(
        "TagResource",
        &json!({"ResourceArn": arn, "Tags": [{"Key": "b", "Value": "2"}]}),
    );
    w.ok(
        "UntagResource",
        &json!({"ResourceArn": arn, "TagKeys": ["a"]}),
    );
    let tags = w.ok("ListTagsForResource", &json!({"ResourceArn": arn}));
    assert_eq!(tags["Tags"], json!([{"Key": "b", "Value": "2"}]));
    let missing = "arn:aws:scheduler:us-east-1:000000000000:schedule-group/nope";
    let (code, _) = w.err("ListTagsForResource", &json!({"ResourceArn": missing}));
    assert_eq!(code, "ResourceNotFoundException");
    let sched = "arn:aws:scheduler:us-east-1:000000000000:schedule/default/x";
    let (code, _) = w.err("ListTagsForResource", &json!({"ResourceArn": sched}));
    assert_eq!(code, "ValidationException");
}
