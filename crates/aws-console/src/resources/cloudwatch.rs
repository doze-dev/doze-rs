//! CloudWatch: metric alarms, and the metrics that have data (a list only: a metric has nothing more to read on a page of its
//! own; its data points are a query, which the console does not make yet).

use super::Format::{Flag, Json, Mono, Named, Number, Pairs, Seconds, Text as Plain, Time};
use super::{
    Apply, Delete, Descriptor, Id, Kind, List, Options, Param, ParamKind, Shape, Step, Tags, Text,
    Write, col, field, op, or, says, state,
};

/// What a tag call names: this alarm, by its ARN.
const ARN: &str = r#"{"ResourceARN":"arn:aws:cloudwatch:{region}:{account}:alarm:{id}"}"#;

/// One alarm among those `DescribeAlarms` answered for its name: a metric alarm, or else a composite one.
macro_rules! alarm {
    ($member:literal) => {
        concat!(
            "MetricAlarms.@AlarmName.",
            $member,
            "|CompositeAlarms.@AlarmName.",
            $member
        )
    };
}

pub(super) static CLOUDWATCH: Descriptor = Descriptor {
    service: "cloudwatch",
    target: "monitoring",
    kinds: &[ALARMS, METRICS],
};

const ALARMS: Kind = Kind {
    id: "alarms",
    title: "Alarms",
    one: "alarm",
    about: "Metric alarms (a metric watched against a threshold) and composite alarms (a rule over other alarms), and what is done when they fire.",
    members: &["AlarmName", "AlarmNames"],
    list: List {
        step: Step::new(
            "DescribeAlarms",
            r#"{"AlarmTypes":["MetricAlarm","CompositeAlarm"],"MaxRecords":100,"NextToken":"{token}"}"#,
        ),
        items: "MetricAlarms|CompositeAlarms",
        item_as: "AlarmName",
        token_out: "NextToken",
        id: "AlarmName",
        name: Text::at("AlarmName"),
        label: None,
        split: None,
    },
    elsewhere: "An alarm is made with its metric, statistic, threshold and actions: use aws cloudwatch put-metric-alarm, or your deploy tool.",
    each: &[],
    columns: &[
        col(
            "Type",
            "$from",
            Named(&[("MetricAlarms", "Metric"), ("CompositeAlarms", "Composite")]),
        ),
        state("State", "StateValue", Plain),
        col("Metric", "MetricName", Mono),
        col("Namespace", "Namespace", Mono),
        col("Threshold", "Threshold", Number),
        col("State changed", "StateUpdatedTimestamp", Time),
    ],
    // CloudWatch has no call that reads one alarm: alarms are asked for by name, and none found is a refusal.
    id_of: Id::Call(
        Step::new(
            "DescribeAlarms",
            r#"{"AlarmNames":["{name}"],"AlarmTypes":["MetricAlarm","CompositeAlarm"]}"#,
        ),
        alarm!("AlarmName"),
    ),
    detail: &[],
    fields: &[
        field("ARN", alarm!("AlarmArn"), Mono),
        field("Description", alarm!("AlarmDescription"), Plain),
        field("State", alarm!("StateValue"), Plain),
        field("Why", alarm!("StateReason"), Plain),
        // A composite alarm has a rule over other alarms in place of a metric and a threshold.
        field("Rule", "CompositeAlarms.@AlarmName.AlarmRule", Mono),
        field("Metric", alarm!("MetricName"), Mono),
        field("Namespace", alarm!("Namespace"), Mono),
        field("Dimensions", alarm!("Dimensions"), Pairs),
        field("Statistic", alarm!("Statistic"), Plain),
        field("Period", alarm!("Period"), Seconds),
        field("Comparison", alarm!("ComparisonOperator"), Mono),
        field("Threshold", alarm!("Threshold"), Number),
        field("Evaluation periods", alarm!("EvaluationPeriods"), Number),
        field("Datapoints to alarm", alarm!("DatapointsToAlarm"), Number),
        field("Missing data", alarm!("TreatMissingData"), Plain),
        field(
            "Actions",
            alarm!("ActionsEnabled"),
            Flag("Enabled", "Disabled"),
        ),
        or("Alarm actions", alarm!("AlarmActions"), Json, "None"),
        or("OK actions", alarm!("OKActions"), Json, "None"),
        field("State changed", alarm!("StateUpdatedTimestamp"), Time),
    ],
    subs: &[],
    create: None,
    delete: Some(Delete {
        verb: "Delete",
        step: Step::new("DeleteAlarms", r#"{"AlarmNames":["{id}"]}"#),
        words: "Deleting removes the alarm and its history. The metric and its data stay.",
    }),
    reveal: None,
    deep: &[],
    tags: Some(Tags::new(
        Step::new("ListTagsForResource", ARN),
        "Tags",
        Shape::KEY_VALUE,
        Write::Delta {
            add: op("TagResource", ARN),
            at: "Tags",
            remove: op("UntagResource", ARN),
            keys: "TagKeys",
        },
    )),
    params: &[
        Param {
            key: "state",
            label: "State",
            input: ParamKind::Chips,
            options: Options::Fixed(&[
                ("", "All"),
                ("ALARM", "In alarm"),
                ("INSUFFICIENT_DATA", "Insufficient data"),
                ("OK", "OK"),
            ]),
            default: "",
            apply: Apply::Member("StateValue"),
        },
        Param {
            key: "type",
            label: "Type",
            input: ParamKind::Select,
            options: Options::Fixed(&[
                ("MetricAlarm", "Metric alarms"),
                ("CompositeAlarm", "Composite alarms"),
            ]),
            default: "",
            apply: Apply::Template(r#"{"AlarmTypes":["{value}"]}"#),
        },
    ],
    page: super::Page {
        badges: Some(says::alarm),
        ..super::Page::NONE
    },
    ..Kind::BARE
};

const METRICS: Kind = Kind {
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
    ..Kind::BARE
};
