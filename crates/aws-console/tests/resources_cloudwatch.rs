//! CloudWatch's pages: dashboards (a kind of their own, read and written whole through the kind's operations), and the
//! operations the alarm and metric pages make as the console.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_lines
)]

mod support;
#[path = "resources/world.rs"]
mod world;

use aws_peers::Mode;
use serde_json::{Value, json};
use world::world;

const BASE: &str = "/api/v1/resources/cloudwatch";

async fn op(w: &world::World, kind: &str, operation: &str, input: Value) -> (u16, Value) {
    w.send("POST", &format!("{BASE}/{kind}/op/{operation}"), &input)
        .await
}

#[tokio::test]
async fn a_dashboard_is_made_listed_read_and_deleted_as_the_console() {
    let w = world("cw-dashboards", Mode::Soft, &[]);
    let body = r##"{"widgets":[{"type":"text","x":0,"y":0,"width":6,"height":3,"properties":{"markdown":"# Runbook"}}]}"##;
    let (status, made) = op(
        &w,
        "dashboards",
        "PutDashboard",
        json!({"DashboardName": "larder-ops", "DashboardBody": body}),
    )
    .await;
    assert_eq!(status, 200, "{made}");
    assert_eq!(made["DashboardValidationMessages"], json!([]));

    let (_, list) = w.get(&format!("{BASE}/dashboards")).await;
    let rows = list["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 1, "{list}");
    assert_eq!(rows[0]["name"], "larder-ops");

    // The body comes back as it was written, from the kind's own GetDashboard and on the resource's page.
    let (_, got) = op(
        &w,
        "dashboards",
        "GetDashboard",
        json!({"DashboardName": "larder-ops"}),
    )
    .await;
    assert_eq!(got["DashboardBody"], body);
    let (status, detail) = w.get(&format!("{BASE}/dashboards/larder-ops")).await;
    assert_eq!(status, 200, "{detail}");
    assert_eq!(world::field(&detail, "Body")["value"], body);

    // A name that is not there is the service's refusal, said as a missing resource; DeleteDashboards takes several names.
    let (status, missing) = w.get(&format!("{BASE}/dashboards/nothing")).await;
    assert_eq!(
        (status, missing["code"].as_str()),
        (400, Some("DashboardNotFoundError")),
        "{missing}"
    );
    let (status, refused) = op(
        &w,
        "dashboards",
        "DeleteDashboards",
        json!({"DashboardNames": ["nothing"]}),
    )
    .await;
    assert_eq!(
        (status, refused["code"].as_str()),
        (400, Some("DashboardNotFoundError")),
        "{refused}"
    );
    let (status, bad) = op(
        &w,
        "dashboards",
        "PutDashboard",
        json!({"DashboardName": "bad name", "DashboardBody": "{}"}),
    )
    .await;
    assert_eq!(
        (status, bad["code"].as_str()),
        (400, Some("DashboardInvalidInputError")),
        "{bad}"
    );
    let (status, gone) = op(
        &w,
        "dashboards",
        "DeleteDashboards",
        json!({"DashboardNames": ["larder-ops"]}),
    )
    .await;
    assert_eq!(status, 200, "{gone}");
    let (_, list) = w.get(&format!("{BASE}/dashboards")).await;
    assert_eq!(list["rows"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn an_alarm_is_made_read_with_its_history_and_deleted_as_the_console_and_nothing_else_is() {
    let w = world("cw-alarms", Mode::Soft, &[]);
    let (status, made) = op(
        &w,
        "alarms",
        "PutMetricAlarm",
        json!({"AlarmName": "dlq-not-empty", "Namespace": "Larder/Queues", "MetricName": "DlqMessages",
            "Dimensions": [{"Name": "QueueName", "Value": "pick-lists-dlq"}], "Statistic": "Maximum", "Period": 60,
            "ComparisonOperator": "GreaterThanOrEqualToThreshold", "Threshold": 1.0, "EvaluationPeriods": 1,
            "DatapointsToAlarm": 1, "TreatMissingData": "missing"}),
    )
    .await;
    assert_eq!(status, 200, "{made}");
    let (_, seen) = op(
        &w,
        "alarms",
        "DescribeAlarms",
        json!({"AlarmNames": ["dlq-not-empty"]}),
    )
    .await;
    let alarm = &seen["MetricAlarms"][0];
    assert_eq!(alarm["Namespace"], "Larder/Queues");
    assert_eq!(alarm["StateValue"], "INSUFFICIENT_DATA");
    let (_, history) = op(
        &w,
        "alarms",
        "DescribeAlarmHistory",
        json!({"AlarmName": "dlq-not-empty"}),
    )
    .await;
    let kinds: Vec<&str> = history["AlarmHistoryItems"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|i| i["HistoryItemType"].as_str())
        .collect();
    assert!(kinds.contains(&"ConfigurationUpdate"), "{history}");

    // Not an operation of the kind: a composite alarm is made elsewhere.
    let (status, refused) = op(
        &w,
        "alarms",
        "PutCompositeAlarm",
        json!({"AlarmName": "x", "AlarmRule": "ALARM(a)"}),
    )
    .await;
    assert_eq!(
        (status, refused["code"].as_str()),
        (404, Some("NOT_IN_PAGE"))
    );

    let (status, gone) = op(
        &w,
        "alarms",
        "DeleteAlarms",
        json!({"AlarmNames": ["dlq-not-empty"]}),
    )
    .await;
    assert_eq!(status, 200, "{gone}");
    assert!(
        w.live()
            .iter()
            .any(|l| l == "console monitoring DeleteAlarms"),
        "{:?}",
        w.live()
    );
}

#[tokio::test]
async fn the_metrics_page_lists_by_namespace_with_the_services_own_listmetrics() {
    let w = world("cw-metrics", Mode::Soft, &[]);
    w.app(
        "monitoring",
        "PutMetricData",
        &json!({"Namespace": "Larder/Orders", "MetricData": [{"MetricName": "OrdersPlaced", "Value": 3.0,
            "Dimensions": [{"Name": "Environment", "Value": "prod"}]}]}),
    )
    .await;
    let (status, listed) = op(
        &w,
        "metrics",
        "ListMetrics",
        json!({"Namespace": "Larder/Orders"}),
    )
    .await;
    assert_eq!(status, 200, "{listed}");
    assert_eq!(listed["Metrics"][0]["MetricName"], "OrdersPlaced");
}

#[tokio::test]
async fn the_dashboards_page_lists_with_the_services_own_listdashboards() {
    let w = world("cw-dash-list", Mode::Soft, &[]);
    op(
        &w,
        "dashboards",
        "PutDashboard",
        json!({"DashboardName": "larder-api", "DashboardBody": "{}"}),
    )
    .await;
    let (status, listed) = op(&w, "dashboards", "ListDashboards", json!({})).await;
    assert_eq!(status, 200, "{listed}");
    assert_eq!(listed["DashboardEntries"][0]["DashboardName"], "larder-api");
    assert!(listed["DashboardEntries"][0]["LastModified"].is_number());
}
