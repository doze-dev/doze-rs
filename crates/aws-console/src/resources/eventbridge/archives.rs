//! EventBridge archives: the list, an archive's page, and what the pages do (create one, replay from it, delete it).

use super::super::Format::{Bytes, Json, Mono, Number, Tail, Text as Plain, Time};
use super::super::{Delete, Field, Id, Kind, LinkTo, List, Step, Text, col, field, ops, or, says};

pub(super) const ARCHIVES: Kind = Kind {
    id: "archives",
    title: "Archives",
    one: "archive",
    about: "Events a bus kept, to replay later.",
    members: &["ArchiveName"],
    list: List {
        step: Step::new("ListArchives", r#"{"Limit":100,"NextToken":"{token}"}"#),
        items: "Archives",
        item_as: "ArchiveName",
        token_out: "NextToken",
        id: "ArchiveName",
        name: Text::at("ArchiveName"),
        label: None,
        split: None,
    },
    elsewhere: "",
    each: &[],
    columns: &[
        col("Source", "EventSourceArn", Tail),
        col("State", "State", Plain),
        col("Events", "EventCount", Number),
        col("Size", "SizeBytes", Bytes),
        super::retention(),
        col("Created", "CreationTime", Time),
    ],
    id_of: Id::Name,
    detail: &[Step::new("DescribeArchive", r#"{"ArchiveName":"{id}"}"#)],
    fields: &[
        field("ARN", "ArchiveArn", Mono),
        Field {
            link: &[LinkTo {
                service: "eventbridge",
                kind: "buses",
                when: None,
            }],
            ..field("Source bus", "EventSourceArn", Tail)
        },
        field("Description", "Description", Plain),
        field("State", "State", Plain),
        field("Why", "StateReason", Plain),
        field("Events", "EventCount", Number),
        field("Size", "SizeBytes", Bytes),
        or(
            "Retention (days)",
            "RetentionDays",
            super::super::Format::Named(&[("0", "Indefinite")]),
            "Indefinite",
        ),
        field("Created", "CreationTime", Time),
        or("Event pattern", "EventPattern", Json, "Every event"),
    ],
    subs: &[],
    create: None,
    delete: Some(Delete {
        verb: "Delete",
        step: Step::new("DeleteArchive", r#"{"ArchiveName":"{id}"}"#),
        words: "Deleting removes the archive and the events in it; they can no longer be replayed.",
    }),
    reveal: None,
    deep: &[],
    // Create archive (a page of its own), and what it and the replay form read: the buses and their rules.
    ops: &[
        ops::free("CreateArchive"),
        ops::free("DeleteArchive"),
        ops::free("ListArchives"),
        ops::free("ListEventBuses"),
    ],
    page: says::eventbridge::ARCHIVE,
    ..Kind::BARE
};
