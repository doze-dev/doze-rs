//! CloudWatch metrics: the ones that have data (a list: a metric has nothing more to read on a page of its own; its data points
//! are a query the pages make with the kind's operations).

use super::super::Format::{Mono, Pairs};
use super::super::{Id, Kind, List, Step, Text, col, ops};

pub(super) const METRICS: Kind = Kind {
    id: "metrics",
    title: "Metrics",
    one: "metric",
    about: "Every metric with data: the services' own and those your code puts. Read only.",
    members: &[],
    list: List {
        step: Step::new("ListMetrics", r#"{"NextToken":"{token}"}"#),
        items: "Metrics",
        item_as: "MetricName",
        token_out: "NextToken",
        // A metric is its namespace, name and dimensions together: the whole row names it.
        id: "",
        name: Text::at(""),
        label: Some(Text::at("MetricName")),
        split: None,
    },
    elsewhere: "Metrics appear when data is put: by the services, or with aws cloudwatch put-metric-data.",
    each: &[],
    columns: &[
        col("Namespace", "Namespace", Mono),
        col("Dimensions", "Dimensions", Pairs),
    ],
    id_of: Id::Name,
    detail: &[],
    fields: &[],
    subs: &[],
    create: None,
    delete: None,
    reveal: None,
    deep: &[],
    // The data points are a query the pages make (a function's Monitor tab, a chart): CloudWatch's own GetMetricData.
    ops: &[ops::free("GetMetricData"), ops::free("ListMetrics")],
    ..Kind::BARE
};
