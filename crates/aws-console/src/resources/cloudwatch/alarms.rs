//! CloudWatch alarms: metric alarms (a metric watched against a threshold) and composite alarms (a rule over other alarms).

use super::super::Format::{Flag, Json, Mono, Named, Number, Pairs, Seconds, Text as Plain, Time};
use super::super::{
    Apply, Delete, Id, Kind, List, Options, Page, Param, ParamKind, Shape, Step, Tags, Text, Write,
    col, field, op, ops, or, says, state,
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

pub(super) const ALARMS: Kind = Kind {
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
    elsewhere: "An alarm is made with its metric, statistic, threshold and actions: choose Create alarm, or use aws cloudwatch put-metric-alarm.",
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
    // The pages read and write alarms with the service's own calls: the alarms as it gives them (the condition, the actions), a
    // create or an edit, a delete of several, and one alarm's history.
    ops: &[
        ops::free("DescribeAlarms"),
        ops::free("PutMetricAlarm"),
        ops::free("DeleteAlarms"),
        ops::free("DescribeAlarmHistory"),
    ],
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
    page: Page {
        badges: Some(says::alarm),
        ..Page::NONE
    },
    ..Kind::BARE
};
