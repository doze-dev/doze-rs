//! What a table's pages (items, indexes, monitor, exports and streams, additional settings) may ask of DynamoDB: one list of
//! operations a tab, each bound to this table (`{"TableName":"{id}"}`, or its ARN for the exports), so a call acts on this table
//! and on no other. A change of a setting is `UpdateTable` with the one member it changes; `UpdateTimeToLive` and the others
//! are DynamoDB's own, bound the same way.

use super::{Deep, Op, op};

/// What every call of a table page names: this table.
pub(super) const TABLE: &str = r#"{"TableName":"{id}"}"#;

/// What an export call names: this table, by its ARN.
const TABLE_ARN: &str = r#"{"TableArn":"arn:aws:dynamodb:{region}:{account}:table/{id}"}"#;

const ITEMS: &[Op] = &[
    // The key schema and the indexes the page queries by.
    op("DescribeTable", TABLE),
    op("Scan", TABLE),
    op("Query", TABLE),
    op("GetItem", TABLE),
    op("PutItem", TABLE),
    op("DeleteItem", TABLE),
];

const INDEXES: &[Op] = &[op("UpdateTable", TABLE)];

const MONITOR: &[Op] = &[
    op("DescribeContributorInsights", TABLE),
    op("UpdateContributorInsights", TABLE),
];

const EXPORTS: &[Op] = &[
    op("UpdateTable", TABLE),
    op("ExportTableToPointInTime", TABLE_ARN),
    op("ListExports", TABLE_ARN),
    op("DescribeKinesisStreamingDestination", TABLE),
    op("EnableKinesisStreamingDestination", TABLE),
    op("DisableKinesisStreamingDestination", TABLE),
    op("DescribeContinuousBackups", TABLE),
    op("UpdateContinuousBackups", TABLE),
];

const SETTINGS: &[Op] = &[
    op("UpdateTable", TABLE),
    op("DescribeTimeToLive", TABLE),
    op("UpdateTimeToLive", TABLE),
];

/// The tabs of a table's page, each with what it may ask.
pub(super) const TABS: &[Deep] = &[
    Deep {
        id: "items",
        title: "Items",
        ops: ITEMS,
    },
    Deep {
        id: "indexes",
        title: "Indexes",
        ops: INDEXES,
    },
    Deep {
        id: "monitor",
        title: "Monitor",
        ops: MONITOR,
    },
    Deep {
        id: "exports",
        title: "Exports and streams",
        ops: EXPORTS,
    },
    Deep {
        id: "settings",
        title: "Additional settings",
        ops: SETTINGS,
    },
];
