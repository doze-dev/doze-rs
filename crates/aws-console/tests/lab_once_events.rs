//! The Lab's forced events that act on a service's own step: an in-flight message that comes back, a schedule that fires, the
//! time-to-live sweep, a cold start. Each against the real service on the bus, through the console's endpoints.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;
#[path = "lab_once/world.rs"]
mod world;

use doze_runtime::Clock as _;
use serde_json::{Value, json};
use support::{keys, ts_keys};
use world::{ONCE, lab, queue, sent};

const R: &str = "us-east-1";

fn choice<'a>(form: &'a Value, value: &str) -> &'a Value {
    form["choices"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["value"] == value)
        .unwrap_or_else(|| panic!("{value} is not offered: {form}"))
}

#[tokio::test]
async fn an_in_flight_message_comes_back_with_its_receive_count_and_a_second_try_says_there_is_none()
 {
    let l = lab("sqs", &[]);
    l.app(R, "sqs", "CreateQueue", &queue("orders")).await;
    l.app(R, "sqs", "CreateQueue", &queue("idle")).await;
    let url = "http://127.0.0.1:4566/000000000000/orders";
    l.app(
        R,
        "sqs",
        "SendMessage",
        &json!({"QueueUrl": url, "MessageBody": "one"}),
    )
    .await;
    let got = l
        .app(R, "sqs", "ReceiveMessage", &json!({"QueueUrl": url}))
        .await;
    let handle = got["Messages"][0]["ReceiptHandle"]
        .as_str()
        .unwrap()
        .to_owned();

    // The form offers the queue that has something in flight first, and says how much.
    let (status, form) = l.get(&format!("{ONCE}/expire-visibility/form")).await;
    assert_eq!(status, 200, "{form}");
    assert_eq!(keys(&form), ts_keys("LabOnceForm"));
    assert_eq!(form["choices"][0]["value"], "orders");
    assert_eq!(form["choices"][0]["detail"], "1 in flight, 0 available.");
    assert_eq!(
        choice(&form, "idle")["detail"],
        "Nothing in flight; 0 available."
    );
    assert!(
        form["preview"]
            .as_str()
            .unwrap()
            .starts_with("Bring back the messages in flight in orders now")
    );
    assert_eq!(form["resting"], false);

    let (status, done) = l
        .force("expire-visibility", &json!({"choice": "orders"}))
        .await;
    assert_eq!(status, 200, "{done}");
    assert_eq!(
        (done["ok"].clone(), done["subject"].clone()),
        (json!(true), json!("orders"))
    );
    assert!(
        sent(&done["sentence"]).starts_with("Brought back 1 message that was in flight in orders"),
        "{done}"
    );
    assert_eq!(done["link"], "/resources/sqs/queues/orders");
    // It is receivable again at once, a second time received, and the first handle still names it.
    let again = l
        .app(
            R,
            "sqs",
            "ReceiveMessage",
            &json!({"QueueUrl": url, "AttributeNames": ["All"]}),
        )
        .await;
    assert_eq!(again["Messages"][0]["Body"], "one");
    assert_eq!(
        again["Messages"][0]["Attributes"]["ApproximateReceiveCount"],
        "2"
    );
    l.app(
        R,
        "sqs",
        "DeleteMessage",
        &json!({"QueueUrl": url, "ReceiptHandle": handle}),
    )
    .await;

    // With nothing in flight the Lab says so, and what the queue holds instead.
    let (_, none) = l
        .force("expire-visibility", &json!({"choice": "orders"}))
        .await;
    assert_eq!(none["ok"], false);
    assert!(
        sent(&none["sentence"]).starts_with("orders has no message in flight"),
        "{none}"
    );
    let (_, missing) = l
        .force("expire-visibility", &json!({"choice": "nope"}))
        .await;
    assert_eq!(missing["ok"], false);
    assert!(
        sent(&missing["sentence"]).starts_with("QueueDoesNotExist: "),
        "{missing}"
    );
    let (_, index) = l.get(ONCE).await;
    assert_eq!(index["history"].as_array().unwrap().len(), 3);
}

#[tokio::test]
async fn a_resting_service_is_described_as_resting_and_read_only_when_asked_to_be() {
    let l = lab("sqs-resting", &[]);
    l.app(R, "sqs", "CreateQueue", &queue("orders")).await;
    assert_eq!(
        l.tasks.rests().rest("sqs", R).await,
        doze_runtime::Rested::Closed
    );
    let before = l.live().len();
    let (_, form) = l.get(&format!("{ONCE}/expire-visibility/form")).await;
    assert_eq!(form["resting"], true, "{form}");
    assert_eq!(form["choices"], json!([]));
    assert!(
        form["note"]
            .as_str()
            .unwrap()
            .starts_with("SQS is resting in us-east-1")
    );
    assert_eq!(l.live().len(), before, "it was not called to draw the form");
    assert_eq!(l.runs("sqs", R), Some(false));
    // Asked to look anyway, it is read (which starts it), and the queue is there.
    let (_, form) = l
        .get(&format!("{ONCE}/expire-visibility/form?look=yes"))
        .await;
    assert_eq!(form["resting"], false);
    assert_eq!(form["choices"][0]["value"], "orders");
    assert_eq!(l.runs("sqs", R), Some(true));
    // Another region has no queue: it says so.
    let (_, form) = l
        .get(&format!(
            "{ONCE}/expire-visibility/form?region=eu-west-1&look=yes"
        ))
        .await;
    assert_eq!(form["empty"], "There is no queue in eu-west-1.");
}

#[tokio::test]
async fn a_schedule_due_in_a_day_fires_now_its_target_gets_the_message_once_and_the_plan_stays() {
    let l = lab("schedule", &[]);
    l.app(R, "sqs", "CreateQueue", &queue("email-outbox")).await;
    let target = |name: &str, state: &str| {
        json!({
            "Name": name, "ScheduleExpression": "cron(0 3 * * ? 2099)", "State": state,
            "FlexibleTimeWindow": {"Mode": "OFF"},
            "Target": {"Arn": "arn:aws:sqs:us-east-1:000000000000:email-outbox",
                "RoleArn": "arn:aws:iam::000000000000:role/scheduler", "Input": "the report"},
        })
    };
    l.app(
        R,
        "scheduler",
        "CreateSchedule",
        &target("nightly-report", "ENABLED"),
    )
    .await;
    l.app(
        R,
        "scheduler",
        "CreateSchedule",
        &target("paused", "DISABLED"),
    )
    .await;

    let (status, form) = l
        .get(&format!(
            "{ONCE}/fire-schedule/form?choice=default%2Fnightly-report"
        ))
        .await;
    assert_eq!(status, 200, "{form}");
    assert_eq!(
        choice(&form, "default/nightly-report")["detail"],
        "ENABLED; target email-outbox"
    );
    assert_eq!(
        choice(&form, "default/paused")["detail"],
        "DISABLED; target email-outbox"
    );
    assert!(
        form["preview"]
            .as_str()
            .unwrap()
            .starts_with("Invoke default/nightly-report's target now")
    );

    let (status, done) = l
        .force(
            "fire-schedule",
            &json!({"choice": "default/nightly-report"}),
        )
        .await;
    assert_eq!(status, 200, "{done}");
    assert_eq!(done["ok"], true, "{done}");
    assert_eq!(
        sent(&done["sentence"]),
        "Fired nightly-report now: it sent a message to email-outbox. Its next scheduled fire is unchanged: 2099-01-01 03:00 UTC."
    );
    assert_eq!(
        done["link"],
        "/resources/scheduler/schedules/default%2Fnightly-report"
    );
    let url = "http://127.0.0.1:4566/000000000000/email-outbox";
    let got = l
        .app(
            R,
            "sqs",
            "ReceiveMessage",
            &json!({"QueueUrl": url, "MaxNumberOfMessages": 10}),
        )
        .await;
    assert_eq!(got["Messages"].as_array().unwrap().len(), 1, "exactly once");
    assert_eq!(got["Messages"][0]["Body"], "the report");
    // Live shows the target's call as the schedule's.
    assert!(
        l.live()
            .iter()
            .any(|c| c.starts_with("sqs SendMessage via ") && c.contains("nightly-report")),
        "{:?}",
        l.live()
    );

    let (_, off) = l
        .force("fire-schedule", &json!({"choice": "default/paused"}))
        .await;
    assert_eq!(off["ok"], false);
    assert!(
        sent(&off["sentence"]).starts_with("paused is disabled"),
        "{off}"
    );
    let (_, gone) = l
        .force("fire-schedule", &json!({"choice": "default/nope"}))
        .await;
    assert_eq!(gone["ok"], false);
    assert!(
        sent(&gone["sentence"]).starts_with("ResourceNotFoundException: "),
        "{gone}"
    );
    let again = l
        .app(R, "sqs", "ReceiveMessage", &json!({"QueueUrl": url}))
        .await;
    assert_eq!(again, json!({}), "the refusals fired nothing");
}

fn table(name: &str) -> Value {
    json!({"TableName": name, "BillingMode": "PAY_PER_REQUEST",
        "KeySchema": [{"AttributeName": "pk", "KeyType": "HASH"}],
        "AttributeDefinitions": [{"AttributeName": "pk", "AttributeType": "S"}]})
}

#[tokio::test]
async fn the_sweep_removes_what_has_expired_and_not_what_has_not() {
    let l = lab("ttl", &[]);
    for name in ["sessions", "plain"] {
        l.app(R, "dynamodb", "CreateTable", &table(name)).await;
    }
    l.app(
        R,
        "dynamodb",
        "UpdateTimeToLive",
        &json!({"TableName": "sessions", "TimeToLiveSpecification": {"Enabled": true, "AttributeName": "exp"}}),
    )
    .await;
    let secs = l.clock.now_ms() / 1000;
    for (pk, s) in [("old", 60), ("new", 7_200)] {
        l.app(
            R,
            "dynamodb",
            "PutItem",
            &json!({"TableName": "sessions", "Item": {"pk": {"S": pk}, "exp": {"N": (secs + s).to_string()}}}),
        )
        .await;
    }
    // Only a table that has time to live is offered.
    let (status, form) = l.get(&format!("{ONCE}/sweep-ttl/form")).await;
    assert_eq!(status, 200, "{form}");
    assert_eq!(form["choices"].as_array().unwrap().len(), 1, "{form}");
    assert_eq!(form["choices"][0]["detail"], "Expiry in the attribute exp.");

    let (_, none) = l.force("sweep-ttl", &json!({"choice": "sessions"})).await;
    assert_eq!(none["ok"], false, "{none}");
    assert!(
        sent(&none["sentence"]).starts_with("No item of sessions has passed its time to live"),
        "{none}"
    );
    assert!(
        sent(&none["sentence"]).contains("The next item expires at 2023-11-14 22:14:20 UTC."),
        "{none}"
    );

    // A minute passes and the timer has not run yet (it sleeps on the real clock): the sweep does what the timer would.
    l.clock.set((secs + 61) * 1000);
    let (_, done) = l.force("sweep-ttl", &json!({"choice": "sessions"})).await;
    assert_eq!(done["ok"], true, "{done}");
    assert!(
        sent(&done["sentence"])
            .starts_with("Ran the time-to-live sweep: removed 1 expired item of sessions."),
        "{done}"
    );
    assert_eq!(done["link"], "/resources/dynamodb/tables/sessions");
    let left = l
        .app(R, "dynamodb", "Scan", &json!({"TableName": "sessions"}))
        .await;
    assert_eq!(left["Count"], 1);
    assert_eq!(left["Items"][0]["pk"]["S"], "new");
    let (_, off) = l.force("sweep-ttl", &json!({"choice": "plain"})).await;
    assert!(
        sent(&off["sentence"]).starts_with("Time to live is off on plain"),
        "{off}"
    );
}

#[tokio::test]
async fn a_function_with_no_process_is_already_cold_and_a_missing_one_is_said() {
    if !world::have_zip() {
        eprintln!("skipped: zip is not installed");
        return;
    }
    let l = lab("cold", &[]);
    l.app(
        R,
        "lambda",
        "CreateFunction",
        &json!({"FunctionName": "report", "Runtime": "provided.al2023", "Handler": "x",
            "Role": "arn:aws:iam::000000000000:role/r", "Code": {"ZipFile": world::function_zip()}}),
    )
    .await;
    let (status, form) = l.get(&format!("{ONCE}/cold-start/form?look=yes")).await;
    assert_eq!(status, 200, "{form}");
    assert_eq!(form["choices"][0]["value"], "report");
    assert_eq!(form["choices"][0]["detail"], "provided.al2023");
    let (_, done) = l.force("cold-start", &json!({"choice": "report"})).await;
    assert_eq!(done["ok"], false, "{done}");
    assert!(
        sent(&done["sentence"]).starts_with("report has no process running in us-east-1"),
        "{done}"
    );
    assert!(
        sent(&done["sentence"]).contains("its next invocation is already a cold one"),
        "{done}"
    );
    let (_, missing) = l.force("cold-start", &json!({"choice": "nope"})).await;
    assert!(
        sent(&missing["sentence"]).starts_with("ResourceNotFoundException: "),
        "{missing}"
    );
}
