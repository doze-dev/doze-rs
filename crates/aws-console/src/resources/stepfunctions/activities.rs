//! Step Functions' activities: the list, one activity's details, and the calls the pages make.

use super::super::Format::{Mono, Time};
use super::super::{Delete, Field, Id, Kind, List, Step, Text, col, field, ops};

pub(super) const ACTIVITIES: Kind = Kind {
    id: "activities",
    title: "Activities",
    one: "activity",
    about: "Tasks done by workers outside Step Functions. A worker polls for work, does it, and reports back.",
    members: &["activityArn"],
    list: List {
        step: Step::new(
            "ListActivities",
            r#"{"maxResults":100,"nextToken":"{token}"}"#,
        ),
        items: "activities",
        item_as: "name",
        token_out: "nextToken",
        id: "activityArn",
        name: Text::at("name"),
        label: None,
        split: None,
    },
    elsewhere: "",
    each: &[],
    columns: &[col("Creation date", "creationDate", Time)],
    id_of: Id::Template("arn:aws:states:{region}:{account}:activity:{name}"),
    detail: &[Step::new("DescribeActivity", r#"{"activityArn":"{id}"}"#)],
    fields: &[
        field("ARN", "activityArn", Mono),
        Field {
            ..field("Created", "creationDate", Time)
        },
    ],
    subs: &[],
    create: None,
    delete: Some(Delete {
        verb: "Delete",
        step: Step::new("DeleteActivity", r#"{"activityArn":"{id}"}"#),
        words: "Deleting removes the activity. A state machine that names it in a Task state fails that task, and a worker polling for it is refused.",
    }),
    reveal: None,
    ops: &[ops::free("CreateActivity")],
    ..Kind::BARE
};
