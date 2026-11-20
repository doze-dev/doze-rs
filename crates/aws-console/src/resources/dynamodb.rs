//! DynamoDB: tables.

use super::Format::{Bytes, Flag, Mono, Number, Text as Plain, Time};
use super::{
    Create, Delete, Descriptor, Id, Input, InputKind, Kind, List, Step, Text, col, field, input,
};

const DESCRIBE: Step = Step::new("DescribeTable", r#"{"TableName":"{id}"}"#);
const TYPES: &[&str] = &["S", "N", "B"];

pub(super) static DYNAMODB: Descriptor = Descriptor {
    service: "dynamodb",
    target: "dynamodb",
    rest: false,
    kinds: &[Kind {
        id: "tables",
        title: "Tables",
        one: "table",
        about: "Tables, their keys, and how much they hold.",
        members: &["TableName", "TableArn"],
        list: List {
            step: Step::new(
                "ListTables",
                r#"{"Limit":100,"ExclusiveStartTableName":"{token}"}"#,
            ),
            items: "TableNames",
            item_as: "TableName",
            token_out: "LastEvaluatedTableName",
            id: "TableName",
            name: Text::at("TableName"),
            label: None,
        },
        each: &[DESCRIBE],
        columns: &[
            col("Status", "Table.TableStatus", Plain),
            col("Partition key", "Table.KeySchema.0.AttributeName", Mono),
            col("Sort key", "Table.KeySchema.1.AttributeName", Mono),
            col("Items", "Table.ItemCount", Number),
            col("Size", "Table.TableSizeBytes", Bytes),
            col("Created", "Table.CreationDateTime", Time),
        ],
        id_of: Id::Name,
        detail: &[
            DESCRIBE,
            Step::new("DescribeTimeToLive", r#"{"TableName":"{id}"}"#).optional(),
        ],
        fields: &[
            field("ARN", "Table.TableArn", Mono),
            field("Status", "Table.TableStatus", Plain),
            field("Partition key", "Table.KeySchema.0.AttributeName", Mono),
            field("Sort key", "Table.KeySchema.1.AttributeName", Mono),
            field(
                "Capacity mode",
                "Table.BillingModeSummary.BillingMode",
                Plain,
            ),
            field(
                "Read capacity units",
                "Table.ProvisionedThroughput.ReadCapacityUnits",
                Number,
            ),
            field(
                "Write capacity units",
                "Table.ProvisionedThroughput.WriteCapacityUnits",
                Number,
            ),
            field("Item count", "Table.ItemCount", Number),
            field("Table size", "Table.TableSizeBytes", Bytes),
            field(
                "Stream",
                "Table.StreamSpecification.StreamEnabled",
                Flag("On", "Off"),
            ),
            field(
                "Stream view type",
                "Table.StreamSpecification.StreamViewType",
                Plain,
            ),
            field("Latest stream ARN", "Table.LatestStreamArn", Mono),
            field(
                "Time to live",
                "TimeToLiveDescription.TimeToLiveStatus",
                Plain,
            ),
            field(
                "Time to live attribute",
                "TimeToLiveDescription.AttributeName",
                Mono,
            ),
            field(
                "Deletion protection",
                "Table.DeletionProtectionEnabled",
                Flag("On", "Off"),
            ),
            field("Created", "Table.CreationDateTime", Time),
        ],
        subs: &[],
        create: Some(Create {
            inputs: &[
                Input {
                    required: true,
                    placeholder: "orders",
                    ..input(
                        "TableName",
                        "Name",
                        "3 to 255 letters, digits, underscores, hyphens and dots.",
                        InputKind::Text,
                    )
                },
                Input {
                    required: true,
                    placeholder: "id",
                    ..input(
                        "PartitionKey",
                        "Partition key",
                        "The attribute every item is found by.",
                        InputKind::Text,
                    )
                },
                Input {
                    required: true,
                    value: "S",
                    ..input(
                        "PartitionKeyType",
                        "Partition key type",
                        "S string, N number, B binary.",
                        InputKind::Select(TYPES),
                    )
                },
                Input {
                    placeholder: "created_at",
                    ..input(
                        "SortKey",
                        "Sort key",
                        "Optional: orders items that share a partition key.",
                        InputKind::Text,
                    )
                },
                Input {
                    value: "S",
                    ..input(
                        "SortKeyType",
                        "Sort key type",
                        "Used only with a sort key.",
                        InputKind::Select(TYPES),
                    )
                },
            ],
            steps: &[Step::new(
                "CreateTable",
                r#"{"TableName":"{form:TableName}","BillingMode":"PAY_PER_REQUEST","AttributeDefinitions":[{"AttributeName":"{form:PartitionKey}","AttributeType":"{form:PartitionKeyType}"},{"AttributeName":"{form:SortKey}","AttributeType":"{form:SortKeyType}"}],"KeySchema":[{"AttributeName":"{form:PartitionKey}","KeyType":"HASH"},{"AttributeName":"{form:SortKey}","KeyType":"RANGE"}]}"#,
            )],
            name: Text::at("TableDescription.TableName"),
            key: "TableName",
        }),
        delete: Some(Delete {
            verb: "Delete",
            step: Step::new("DeleteTable", r#"{"TableName":"{id}"}"#),
            words: "Deleting removes the table and every item in it. Its stream, if it has one, stops.",
        }),
        reveal: None,
    }],
};
