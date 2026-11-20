//! The actions AWS tracks per service in an `ACTION_LEVEL` service last accessed report, as recorded.

/// DynamoDB's tracked actions (recorded 2026-11-19).
const TRACKED_DYNAMODB: &[&str] = &[
    "CreateBackup",
    "CreateGlobalTable",
    "CreateTable",
    "DeleteBackup",
    "DeleteTable",
    "DescribeBackup",
    "DescribeContinuousBackups",
    "DescribeContributorInsights",
    "DescribeEndpoints",
    "DescribeExport",
    "DescribeGlobalTable",
    "DescribeGlobalTableSettings",
    "DescribeImport",
    "DescribeKinesisStreamingDestination",
    "DescribeLimits",
    "DescribeStream",
    "DescribeTable",
    "DescribeTableReplicaAutoScaling",
    "DescribeTimeToLive",
    "DisableKinesisStreamingDestination",
    "EnableKinesisStreamingDestination",
    "ExportTableToPointInTime",
    "GetResourcePolicy",
    "ImportTable",
    "ListBackups",
    "ListBulkOperations",
    "ListContributorInsights",
    "ListExports",
    "ListGlobalTables",
    "ListImports",
    "ListStreams",
    "ListTables",
    "RestoreTableFromBackup",
    "RestoreTableToPointInTime",
    "UpdateContinuousBackups",
    "UpdateContributorInsights",
    "UpdateGlobalTable",
    "UpdateGlobalTableSettings",
    "UpdateKinesisStreamingDestination",
    "UpdateTable",
    "UpdateTableReplicaAutoScaling",
    "UpdateTimeToLive",
];
/// S3's tracked actions (recorded 2026-11-19).
const TRACKED_S3: &[&str] = &[
    "GetAccelerateConfiguration",
    "GetAccessGrant",
    "GetAccessGrantsInstance",
    "GetAccessGrantsInstanceForPrefix",
    "GetAccessGrantsInstanceResourcePolicy",
    "GetAccessGrantsLocation",
    "GetAccessPoint",
    "GetAccessPointConfigurationForObjectLambda",
    "GetAccessPointForObjectLambda",
    "GetAccessPointPolicy",
    "GetAccessPointPolicyForObjectLambda",
    "GetAccessPointPolicyStatus",
    "GetAccessPointPolicyStatusForObjectLambda",
    "GetAccountPublicAccessBlock",
    "GetAnalyticsConfiguration",
    "GetBucketAbac",
    "GetBucketAcl",
    "GetBucketCORS",
    "GetBucketLocation",
    "GetBucketLogging",
    "GetBucketNotification",
    "GetBucketObjectLockConfiguration",
    "GetBucketOwnershipControls",
    "GetBucketPolicy",
    "GetBucketPolicyStatus",
    "GetBucketPublicAccessBlock",
    "GetBucketRequestPayment",
    "GetBucketVersioning",
    "GetBucketWebsite",
    "GetDataAccess",
    "GetEncryptionConfiguration",
    "GetIntelligentTieringConfiguration",
    "GetInventoryConfiguration",
    "GetLifecycleConfiguration",
    "GetMetricsConfiguration",
    "GetMultiRegionAccessPoint",
    "GetMultiRegionAccessPointPolicy",
    "GetMultiRegionAccessPointPolicyStatus",
    "GetMultiRegionAccessPointRoutes",
    "GetReplicationConfiguration",
    "GetStorageLensConfiguration",
    "GetStorageLensDashboard",
];

/// The actions AWS tracks for a service in an `ACTION_LEVEL` report, listed whether asked for or not. Recorded 2026-11-19 for
/// DynamoDB and S3 (42 each, in this order); AWS tracks others (IAM, EC2, Lambda) whose lists were not recorded.
pub(crate) fn tracked_actions(namespace: &str) -> &'static [&'static str] {
    match namespace {
        "dynamodb" => TRACKED_DYNAMODB,
        "s3" => TRACKED_S3,
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    /// The report lists tracked actions in sorted order; the recorded lists must already be in it, so none is reordered.
    #[test]
    fn the_recorded_lists_are_in_the_order_aws_gave_them() {
        for ns in ["dynamodb", "s3"] {
            let list = super::tracked_actions(ns);
            assert_eq!(list.len(), 42, "{ns}");
            assert!(list.is_sorted(), "{ns}");
        }
    }
}
