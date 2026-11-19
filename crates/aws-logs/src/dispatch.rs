//! Which operations are built, and the dispatch from an operation to the code that does it.

use aws_model_cloudwatch_logs::Operation;
use aws_model_cloudwatch_logs::tables::TABLES;
use aws_wire::ApiError;
use aws_wire::handler::reply_json;
use aws_wire::service::{Call, Reply};
use serde_json::Value;

use crate::core::{Core, not_built};
use crate::model::Group;
use crate::req::Req;

/// The operations this service runs. Every other is refused by name before its request is read.
pub(crate) fn built(op: Operation) -> bool {
    use Operation as O;
    matches!(
        op,
        O::CreateLogGroup
            | O::DeleteLogGroup
            | O::DescribeLogGroups
            | O::ListLogGroups
            | O::CreateLogStream
            | O::DeleteLogStream
            | O::DescribeLogStreams
            | O::PutLogEvents
            | O::GetLogEvents
            | O::FilterLogEvents
            | O::PutRetentionPolicy
            | O::DeleteRetentionPolicy
            | O::TagLogGroup
            | O::UntagLogGroup
            | O::ListTagsLogGroup
            | O::TagResource
            | O::UntagResource
            | O::ListTagsForResource
            | O::AssociateKmsKey
            | O::DisassociateKmsKey
            | O::PutLogGroupDeletionProtection
            | O::PutMetricFilter
            | O::DeleteMetricFilter
            | O::DescribeMetricFilters
            | O::TestMetricFilter
            | O::PutSubscriptionFilter
            | O::DeleteSubscriptionFilter
            | O::DescribeSubscriptionFilters
    ) || built_documents(op)
        || built_later(op)
}

fn built_documents(op: Operation) -> bool {
    use Operation as O;
    matches!(
        op,
        O::PutResourcePolicy
            | O::DescribeResourcePolicies
            | O::DeleteResourcePolicy
            | O::PutAccountPolicy
            | O::DescribeAccountPolicies
            | O::DeleteAccountPolicy
            | O::PutQueryDefinition
            | O::DescribeQueryDefinitions
            | O::DeleteQueryDefinition
            | O::PutDataProtectionPolicy
            | O::GetDataProtectionPolicy
            | O::DeleteDataProtectionPolicy
    )
}

/// Insights, Live Tail, exports, destinations, transformers, field indexes and scheduled queries.
fn built_later(op: Operation) -> bool {
    use Operation as O;
    matches!(
        op,
        O::StartQuery
            | O::GetQueryResults
            | O::StopQuery
            | O::DescribeQueries
            | O::ListLogGroupsForQuery
            | O::GetLogRecord
            | O::GetLogGroupFields
            | O::StartLiveTail
            | O::CreateExportTask
            | O::CancelExportTask
            | O::DescribeExportTasks
            | O::PutDestination
            | O::PutDestinationPolicy
            | O::DeleteDestination
            | O::DescribeDestinations
            | O::PutTransformer
            | O::GetTransformer
            | O::DeleteTransformer
            | O::TestTransformer
            | O::PutIndexPolicy
            | O::DeleteIndexPolicy
            | O::DescribeIndexPolicies
            | O::DescribeFieldIndexes
            | O::CreateScheduledQuery
            | O::GetScheduledQuery
            | O::UpdateScheduledQuery
            | O::DeleteScheduledQuery
            | O::ListScheduledQueries
            | O::GetScheduledQueryHistory
    )
}

impl Core {
    /// Runs the call and writes the answer as the operation's output.
    pub(crate) fn dispatch(&mut self, call: &Call) -> Reply {
        let Some(op) = Operation::from_index(call.op).filter(|op| built(*op)) else {
            return Err(not_built());
        };
        let r = Req::new(call);
        let value = self.run(op, &r)?;
        reply_json(&TABLES, &self.patterns, call.op, &value)
    }

    fn run(&mut self, op: Operation, r: &Req<'_>) -> Result<Value, ApiError> {
        use Operation as O;
        match op {
            O::CreateLogGroup => self.create_group(r),
            O::DeleteLogGroup => self.delete_group(r),
            O::DescribeLogGroups => self.describe_groups(r),
            O::ListLogGroups => Ok(self.list_groups(r)),
            O::CreateLogStream => self.create_stream(r),
            O::DeleteLogStream => self.delete_stream(r),
            O::DescribeLogStreams => self.describe_streams(r),
            O::PutLogEvents => self.put_events(r),
            O::GetLogEvents => self.get_events(r),
            O::FilterLogEvents => self.filter_events(r),
            O::PutRetentionPolicy => self.put_retention(r),
            O::DeleteRetentionPolicy => self.delete_retention(r),
            O::TagLogGroup | O::TagResource => self.tag(r),
            O::UntagLogGroup | O::UntagResource => self.untag(r),
            O::ListTagsLogGroup | O::ListTagsForResource => self.list_tags(r),
            O::AssociateKmsKey => self.associate_key(r),
            O::DisassociateKmsKey => self.disassociate_key(r),
            O::PutLogGroupDeletionProtection => self.set_protection(r),
            O::PutMetricFilter => self.put_metric_filter(r),
            O::DeleteMetricFilter => self.delete_metric_filter(r),
            O::DescribeMetricFilters => self.describe_metric_filters(r),
            O::TestMetricFilter => Self::test_metric_filter(r),
            O::PutSubscriptionFilter => self.put_subscription(r),
            O::DeleteSubscriptionFilter => self.delete_subscription(r),
            O::DescribeSubscriptionFilters => self.describe_subscriptions(r),
            other if built_later(other) => self.run_later(other, r),
            other => self.run_docs(other, r),
        }
    }

    fn run_later(&mut self, op: Operation, r: &Req<'_>) -> Result<Value, ApiError> {
        use Operation as O;
        match op {
            O::StartQuery => self.start_query(r),
            O::GetQueryResults => self.get_query_results(r),
            O::StopQuery => self.stop_query(r),
            O::DescribeQueries => Ok(self.describe_queries(r)),
            O::ListLogGroupsForQuery => self.list_groups_for_query(r),
            O::GetLogRecord => self.get_log_record(r),
            O::GetLogGroupFields => self.get_log_group_fields(r),
            O::CreateExportTask => self.create_export(r),
            O::CancelExportTask => self.cancel_export(r),
            O::DescribeExportTasks => Ok(self.describe_exports(r)),
            O::PutDestination => self.put_destination(r),
            O::PutDestinationPolicy => self.put_destination_policy(r),
            O::DeleteDestination => self.delete_destination(r),
            O::DescribeDestinations => Ok(self.describe_destinations(r)),
            O::PutTransformer => self.put_transformer(r),
            O::GetTransformer => self.get_transformer(r),
            O::DeleteTransformer => self.delete_transformer(r),
            O::TestTransformer => Self::test_transformer(r),
            O::PutIndexPolicy => self.put_index_policy(r),
            O::DeleteIndexPolicy => self.delete_index_policy(r),
            O::DescribeIndexPolicies => self.describe_index_policies(r),
            O::DescribeFieldIndexes => self.describe_field_indexes(r),
            O::CreateScheduledQuery => self.create_scheduled(r),
            O::GetScheduledQuery => self.get_scheduled(r),
            O::UpdateScheduledQuery => self.update_scheduled(r),
            O::DeleteScheduledQuery => self.delete_scheduled(r),
            O::ListScheduledQueries => Ok(self.list_scheduled(r)),
            O::GetScheduledQueryHistory => self.scheduled_history(r),
            // `StartLiveTail` is answered as a stream (`Logs::call_stream`), never here.
            _ => Err(not_built()),
        }
    }

    /// The group the request names.
    pub(crate) fn group(&self, r: &Req<'_>) -> Result<&Group, ApiError> {
        let name = r.group_name().unwrap_or_default();
        self.groups.get(&name).ok_or_else(crate::req::no_group)
    }

    pub(crate) fn group_mut(&mut self, r: &Req<'_>) -> Result<&mut Group, ApiError> {
        let name = r.group_name().unwrap_or_default();
        self.groups.get_mut(&name).ok_or_else(crate::req::no_group)
    }
}
