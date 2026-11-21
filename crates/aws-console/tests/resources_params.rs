//! A list's parameters (`?p.<key>=<value>`): filters and selectors the server applies to the service's own list call, against
//! real services on a real bus.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_lines,
    clippy::cognitive_complexity
)]

mod support;
#[path = "resources/world.rs"]
mod world;

use aws_peers::Mode;
use serde_json::{Value, json};
use support::{keys, ts_keys};
use world::{encode, world};

const BASE: &str = "/api/v1/resources";

fn row_names(list: &Value) -> Vec<String> {
    list["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["name"].as_str().unwrap().to_owned())
        .collect()
}

fn param<'a>(list: &'a Value, key: &str) -> &'a Value {
    list["params"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["key"] == key)
        .unwrap_or_else(|| panic!("no param {key} in {list}"))
}

fn values(p: &Value) -> Vec<(String, Option<u64>)> {
    p["options"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| (o["value"].as_str().unwrap().to_owned(), o["count"].as_u64()))
        .collect()
}

#[tokio::test]
async fn rules_are_listed_for_the_bus_a_selector_names_from_the_buses_there_are() {
    let w = world("params-bus", Mode::Soft, &[]);
    w.app("events", "CreateEventBus", &json!({"Name": "larder"}))
        .await;
    w.app(
        "events",
        "PutRule",
        &json!({"Name": "nightly", "ScheduleExpression": "rate(1 day)"}),
    )
    .await;
    w.app(
        "events",
        "PutRule",
        &json!({"Name": "on-order", "EventBusName": "larder", "EventPattern": "{\"source\":[\"x\"]}"}),
    )
    .await;
    let (status, all) = w.get(&format!("{BASE}/eventbridge/rules")).await;
    assert_eq!(status, 200, "{all}");
    assert_eq!(keys(&all), ts_keys("ResourceList"));
    let bus = param(&all, "bus");
    assert_eq!(keys(bus), ts_keys("ParamSpec"));
    assert_eq!(
        (bus["input"].clone(), bus["value"].clone()),
        (json!("select"), Value::Null)
    );
    assert_eq!(
        values(bus),
        [("default".to_owned(), None), ("larder".to_owned(), None)]
    );
    assert_eq!(keys(&bus["options"][0]), ts_keys("ParamOption"));
    let (_, chosen) = w
        .get(&format!("{BASE}/eventbridge/rules?p.bus=larder"))
        .await;
    assert_eq!(row_names(&chosen), ["larder/on-order"]);
    assert_eq!(param(&chosen, "bus")["value"], "larder");
    let (_, default) = w
        .get(&format!("{BASE}/eventbridge/rules?p.bus=default"))
        .await;
    assert_eq!(row_names(&default), ["nightly"]);
    // A key the kind does not have is ignored, and a kind with no parameters lists none.
    let (_, same) = w.get(&format!("{BASE}/eventbridge/rules?p.nope=1")).await;
    assert_eq!(row_names(&same), row_names(&all));
    let (_, buses) = w.get(&format!("{BASE}/eventbridge/buses")).await;
    assert_eq!(buses["params"], json!([]));
}

#[tokio::test]
async fn policies_are_listed_by_scope_and_secrets_and_parameters_by_what_their_names_begin_with() {
    let w = world("params-names", Mode::Soft, &[]);
    let doc = json!({"Version": "2012-10-17", "Statement": [{"Effect": "Allow", "Action": "s3:GetObject", "Resource": "*"}]});
    w.app(
        "iam",
        "CreatePolicy",
        &json!({"PolicyName": "mine", "PolicyDocument": doc.to_string()}),
    )
    .await;
    let (_, local) = w.get(&format!("{BASE}/iam/policies")).await;
    let scope = param(&local, "scope");
    assert_eq!(
        (scope["input"].clone(), scope["value"].clone()),
        (json!("chips"), json!("Local"))
    );
    assert_eq!(
        values(scope)
            .iter()
            .map(|(v, _)| v.as_str())
            .collect::<Vec<_>>(),
        ["Local", "AWS", "All"]
    );
    assert_eq!(row_names(&local).len(), 1);
    let (_, aws) = w.get(&format!("{BASE}/iam/policies?p.scope=AWS")).await;
    assert!(
        !row_names(&aws).iter().any(|n| n.ends_with(":policy/mine")),
        "{aws}"
    );
    let (_, all) = w.get(&format!("{BASE}/iam/policies?p.scope=All")).await;
    assert!(row_names(&all).len() >= row_names(&local).len());

    for name in ["app/db", "app/cache", "other"] {
        w.app(
            "secretsmanager",
            "CreateSecret",
            &json!({"Name": name, "SecretString": "x"}),
        )
        .await;
    }
    let (_, secrets) = w
        .get(&format!("{BASE}/secretsmanager/secrets?p.name=app"))
        .await;
    assert_eq!(row_names(&secrets), ["app/cache", "app/db"]);
    assert_eq!(param(&secrets, "name")["input"], "text");

    for name in ["/app/a", "/app/b", "/other/c"] {
        w.app(
            "ssm",
            "PutParameter",
            &json!({"Name": name, "Type": "String", "Value": "1"}),
        )
        .await;
    }
    let (_, params) = w
        .get(&format!(
            "{BASE}/ssm/parameters?p.begins-with={}",
            encode("/app/")
        ))
        .await;
    assert_eq!(row_names(&params), ["/app/a", "/app/b"]);
    let (_, none) = w
        .get(&format!("{BASE}/ssm/parameters?p.begins-with=%2Fzzz"))
        .await;
    assert_eq!(row_names(&none), Vec::<String>::new());
    let (_, every) = w.get(&format!("{BASE}/ssm/parameters")).await;
    assert_eq!(row_names(&every).len(), 3);
}

#[tokio::test]
async fn alarms_are_listed_by_state_and_type_and_stacks_by_a_group_of_statuses_with_their_counts() {
    let w = world("params-alarms", Mode::Soft, &[]);
    w.app(
        "monitoring",
        "PutMetricAlarm",
        &json!({"AlarmName": "queue-deep", "MetricName": "ApproximateNumberOfMessagesVisible",
            "Namespace": "AWS/SQS", "Statistic": "Maximum", "Period": 60, "EvaluationPeriods": 1,
            "Threshold": 100.0, "ComparisonOperator": "GreaterThanThreshold"}),
    )
    .await;
    let (_, list) = w.get(&format!("{BASE}/cloudwatch/alarms")).await;
    assert_eq!(row_names(&list), ["queue-deep"]);
    assert_eq!(
        values(param(&list, "state"))
            .iter()
            .map(|(v, _)| v.as_str())
            .collect::<Vec<_>>(),
        ["", "ALARM", "INSUFFICIENT_DATA", "OK"]
    );
    let state = row_state(&list);
    let (_, same) = w
        .get(&format!("{BASE}/cloudwatch/alarms?p.state={state}"))
        .await;
    assert_eq!(row_names(&same), ["queue-deep"]);
    let other = if state == "OK" { "ALARM" } else { "OK" };
    let (_, none) = w
        .get(&format!("{BASE}/cloudwatch/alarms?p.state={other}"))
        .await;
    assert_eq!(row_names(&none), Vec::<String>::new());
    let (_, metric) = w
        .get(&format!("{BASE}/cloudwatch/alarms?p.type=MetricAlarm"))
        .await;
    assert_eq!(row_names(&metric), ["queue-deep"]);
    assert_eq!(param(&metric, "type")["value"], "MetricAlarm");
    let (_, composite) = w
        .get(&format!("{BASE}/cloudwatch/alarms?p.type=CompositeAlarm"))
        .await;
    assert_eq!(row_names(&composite), Vec::<String>::new());
    // The state column is of states, shown as badges.
    assert_eq!(list["columns"][0]["badge"], Value::Null);
    assert_eq!(list["columns"][1]["badge"], "state");
    assert_eq!(list["columns"][2]["badge"], Value::Null);

    let template =
        json!({"Resources": {"Q": {"Type": "AWS::SQS::Queue", "Properties": {"QueueName": "q"}}}});
    for name in ["one", "two"] {
        w.app(
            "cloudformation",
            "CreateStack",
            &json!({"StackName": name, "TemplateBody": template.to_string().replace("\"q\"", &format!("\"q-{name}\""))}),
        )
        .await;
    }
    for _ in 0..100 {
        let (_, l) = w.get(&format!("{BASE}/cloudformation/stacks")).await;
        if values(param(&l, "status"))
            .iter()
            .any(|(v, c)| v == "COMPLETE" && *c == Some(2))
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    let (_, stacks) = w.get(&format!("{BASE}/cloudformation/stacks")).await;
    assert_eq!(
        values(param(&stacks, "status")),
        [
            (String::new(), Some(2)),
            ("COMPLETE".to_owned(), Some(2)),
            ("IN_PROGRESS".to_owned(), Some(0)),
            ("FAILED".to_owned(), Some(0)),
            ("ROLLBACK".to_owned(), Some(0)),
        ]
    );
    let (_, complete) = w
        .get(&format!("{BASE}/cloudformation/stacks?p.status=COMPLETE"))
        .await;
    assert_eq!(row_names(&complete).len(), 2);
    let (_, failed) = w
        .get(&format!("{BASE}/cloudformation/stacks?p.status=FAILED"))
        .await;
    assert_eq!(row_names(&failed), Vec::<String>::new());
    // The counts are of the page, not of what the filter left.
    assert_eq!(
        values(param(&failed, "status"))[1],
        ("COMPLETE".to_owned(), Some(2))
    );
}

/// The alarm's state as its row shows it.
fn row_state(list: &Value) -> String {
    list["rows"][0]["cells"][1].as_str().unwrap().to_owned()
}
