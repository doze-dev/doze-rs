//! Where a call goes: the operations this service runs, and the dispatch of the backup, export, import, policy and streaming
//! operations, and of what work done outside the actor found.

use aws_model_dynamodb::Operation;
use aws_wire::ApiError;
use serde_json::Value as Json;

use crate::core::{Core, Cx};
use crate::work::Done;

impl Core {
    /// Backups and restores, exports and imports, resource policies and Kinesis streaming destinations.
    pub(crate) fn dispatch_more(&mut self, op: Operation, cx: &Cx<'_>) -> Result<Json, ApiError> {
        match op {
            Operation::CreateBackup => self.create_backup(cx),
            Operation::DeleteBackup => self.delete_backup(cx),
            Operation::DescribeBackup => self.describe_backup(cx),
            Operation::ListBackups => self.list_backups(cx),
            Operation::RestoreTableFromBackup => self.restore_from_backup(cx),
            Operation::RestoreTableToPointInTime => self.restore_to_point_in_time(cx),
            Operation::ExportTableToPointInTime => self.export_table(cx),
            Operation::DescribeExport => self.describe_export(cx),
            Operation::ListExports => self.list_exports(cx),
            Operation::ImportTable => self.import_table(cx),
            Operation::DescribeImport => self.describe_import(cx),
            Operation::ListImports => self.list_imports(cx),
            Operation::PutResourcePolicy => self.put_resource_policy(cx),
            Operation::GetResourcePolicy => self.get_resource_policy(cx),
            Operation::DeleteResourcePolicy => self.delete_resource_policy(cx),
            Operation::EnableKinesisStreamingDestination => self.enable_kinesis(cx),
            Operation::DisableKinesisStreamingDestination => self.disable_kinesis(cx),
            Operation::DescribeKinesisStreamingDestination => self.describe_kinesis(cx),
            Operation::UpdateKinesisStreamingDestination => self.update_kinesis(cx),
            _ => self.dispatch_items(op, cx),
        }
    }

    pub(crate) fn dispatch_items(&mut self, op: Operation, cx: &Cx<'_>) -> Result<Json, ApiError> {
        match op {
            Operation::PutItem => self.put_item(cx),
            Operation::GetItem => self.get_item(cx),
            Operation::UpdateItem => self.update_item(cx),
            Operation::DeleteItem => self.delete_item(cx),
            Operation::Query => self.query(cx),
            Operation::Scan => self.scan(cx),
            Operation::BatchGetItem => self.batch_get(cx),
            Operation::BatchWriteItem => self.batch_write(cx),
            Operation::TransactGetItems => self.transact_get(cx),
            Operation::TransactWriteItems => self.transact_write(cx),
            Operation::ExecuteStatement => self.execute_statement(cx),
            Operation::BatchExecuteStatement => self.batch_execute_statement(cx),
            Operation::ExecuteTransaction => self.execute_transaction(cx),
            _ => Err(not_built()),
        }
    }

    /// Takes in what work done outside the actor found.
    pub(crate) fn apply_done(&mut self, done: Done, now_ms: i64) -> Result<(), ApiError> {
        match done {
            Done::Stream {
                table,
                stream_arn,
                found,
            } => {
                self.stream_checked(&table, &stream_arn, found);
                Ok(())
            }
            Done::Exported { arn, result } => {
                self.exported(&arn, result, now_ms);
                Ok(())
            }
            Done::Imported { arn, result } => self.imported(&arn, result, now_ms),
        }
    }
}

/// The operations this service runs. Every other is refused by name before its request is read.
pub(crate) fn built(op: Operation) -> bool {
    matches!(
        op,
        Operation::CreateTable
            | Operation::DescribeTable
            | Operation::DeleteTable
            | Operation::ListTables
            | Operation::UpdateTable
            | Operation::DescribeTimeToLive
            | Operation::UpdateTimeToLive
            | Operation::DescribeContinuousBackups
            | Operation::UpdateContinuousBackups
            | Operation::TagResource
            | Operation::UntagResource
            | Operation::ListTagsOfResource
            | Operation::DescribeLimits
            | Operation::DescribeEndpoints
            | Operation::PutItem
            | Operation::GetItem
            | Operation::UpdateItem
            | Operation::DeleteItem
            | Operation::Query
            | Operation::Scan
            | Operation::BatchGetItem
            | Operation::BatchWriteItem
            | Operation::TransactGetItems
            | Operation::TransactWriteItems
            | Operation::DescribeContributorInsights
            | Operation::UpdateContributorInsights
            | Operation::ListContributorInsights
            | Operation::ExecuteStatement
            | Operation::BatchExecuteStatement
            | Operation::ExecuteTransaction
            | Operation::CreateBackup
            | Operation::DeleteBackup
            | Operation::DescribeBackup
            | Operation::ListBackups
            | Operation::RestoreTableFromBackup
            | Operation::RestoreTableToPointInTime
            | Operation::ExportTableToPointInTime
            | Operation::DescribeExport
            | Operation::ListExports
            | Operation::ImportTable
            | Operation::DescribeImport
            | Operation::ListImports
            | Operation::PutResourcePolicy
            | Operation::GetResourcePolicy
            | Operation::DeleteResourcePolicy
            | Operation::EnableKinesisStreamingDestination
            | Operation::DisableKinesisStreamingDestination
            | Operation::DescribeKinesisStreamingDestination
            | Operation::UpdateKinesisStreamingDestination
    )
}

pub(crate) fn not_built() -> ApiError {
    ApiError::new(
        501,
        "NotImplemented",
        "This operation is not built in doze-aws.",
    )
}

/// The default quotas, as the developer guide gives them.
pub(crate) fn limits() -> Json {
    serde_json::json!({
        "AccountMaxReadCapacityUnits": 80_000,
        "AccountMaxWriteCapacityUnits": 80_000,
        "TableMaxReadCapacityUnits": 40_000,
        "TableMaxWriteCapacityUnits": 40_000,
    })
}

/// The address the caller used, since an SDK that discovers endpoints must be sent back to the same place.
pub(crate) fn endpoints(cx: &Cx<'_>) -> Json {
    let endpoint = &cx.call.caller.endpoint;
    let host = endpoint
        .split_once("://")
        .map_or(endpoint.as_str(), |(_, h)| h);
    serde_json::json!({"Endpoints": [{"Address": host, "CachePeriodInMinutes": 1440}]})
}
