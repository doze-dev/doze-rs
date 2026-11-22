//! CloudWatch dashboards: a name and a body of widgets that doze-aws keeps exactly as written (`crates/aws-cloudwatch/src/docs.rs`:
//! nothing reads the body but the pages that draw it).

use super::super::Format::{Mono, Number, Text as Plain, Time};
use super::super::{Delete, Id, Kind, List, Step, Text, col, field, ops};

pub(super) const DASHBOARDS: Kind = Kind {
    id: "dashboards",
    title: "Dashboards",
    one: "dashboard",
    about: "Saved arrangements of graphs, numbers, alarm states and notes. doze-aws keeps the body as written and never interprets it.",
    members: &["DashboardName", "DashboardNames"],
    list: List {
        step: Step::new("ListDashboards", r#"{"NextToken":"{token}"}"#),
        items: "DashboardEntries",
        item_as: "DashboardName",
        token_out: "NextToken",
        id: "DashboardName",
        name: Text::at("DashboardName"),
        label: None,
        split: None,
    },
    elsewhere: "A dashboard is made with Create dashboard, or with aws cloudwatch put-dashboard.",
    each: &[],
    columns: &[
        col("Last modified", "LastModified", Time),
        col("Size", "Size", Number),
    ],
    // No call reads a dashboard by listing: GetDashboard answers one, and a name that is not there is its refusal.
    id_of: Id::Call(
        Step::new("GetDashboard", r#"{"DashboardName":"{name}"}"#),
        "DashboardName",
    ),
    detail: &[],
    fields: &[
        field("ARN", "DashboardArn", Mono),
        field("Body", "DashboardBody", Plain),
    ],
    subs: &[],
    create: None,
    delete: Some(Delete {
        verb: "Delete",
        step: Step::new("DeleteDashboards", r#"{"DashboardNames":["{id}"]}"#),
        words: "Deleting removes the dashboard and its widgets. The metrics and alarms it showed stay.",
    }),
    reveal: None,
    deep: &[],
    // The pages read a dashboard's body to draw it, and write it back whole (PutDashboard replaces the entire body).
    ops: &[
        ops::free("GetDashboard"),
        ops::free("PutDashboard"),
        ops::free("DeleteDashboards"),
    ],
    ..Kind::BARE
};
