//! Per-type facts from the resource schemas (memory, from the published registry schemas): which properties force a new physical
//! resource when they change (`createOnlyProperties`), which property names a resource, and its primary identifier.

/// The properties of `ty` whose change replaces the resource.
pub(crate) fn create_only(ty: &str) -> &'static [&'static str] {
    match ty {
        "AWS::SQS::Queue" => &["QueueName", "FifoQueue"],
        "AWS::SQS::QueueInlinePolicy" => &["Queue"],
        "AWS::SNS::Topic" => &["TopicName", "FifoTopic"],
        "AWS::SNS::Subscription" => &["Endpoint", "Protocol", "TopicArn", "Region"],
        "AWS::SNS::TopicInlinePolicy" => &["TopicArn"],
        "AWS::SSM::Parameter" => &["Name"],
        "AWS::S3::Bucket" => &["BucketName", "ObjectLockEnabled"],
        "AWS::S3::BucketPolicy" => &["Bucket"],
        "AWS::DynamoDB::Table" | "AWS::DynamoDB::GlobalTable" => {
            &["TableName", "KeySchema", "LocalSecondaryIndexes"]
        }
        "AWS::IAM::Role" => &["RoleName", "Path"],
        "AWS::IAM::User" => &["UserName"],
        "AWS::IAM::Group" => &["GroupName"],
        "AWS::IAM::ManagedPolicy" => &["ManagedPolicyName", "Path", "Description"],
        "AWS::IAM::InstanceProfile" => &["InstanceProfileName", "Path"],
        "AWS::IAM::AccessKey" => &["UserName", "Serial"],
        "AWS::IAM::ServiceLinkedRole" => &["AWSServiceName", "CustomSuffix"],
        "AWS::IAM::OIDCProvider" => &["Url"],
        "AWS::IAM::SAMLProvider" => &["Name"],
        "AWS::IAM::RolePolicy" => &["PolicyName", "RoleName"],
        "AWS::IAM::UserPolicy" => &["PolicyName", "UserName"],
        "AWS::IAM::GroupPolicy" => &["PolicyName", "GroupName"],
        "AWS::Logs::LogGroup" => &["LogGroupName", "LogGroupClass"],
        "AWS::Logs::LogStream" => &["LogGroupName", "LogStreamName"],
        "AWS::Logs::MetricFilter" | "AWS::Logs::SubscriptionFilter" => {
            &["FilterName", "LogGroupName"]
        }
        "AWS::Logs::ResourcePolicy" => &["PolicyName"],
        _ => create_only_more(ty),
    }
}

/// [`create_only`] for the types after Logs.
fn create_only_more(ty: &str) -> &'static [&'static str] {
    match ty {
        "AWS::Lambda::Function" => &["FunctionName"],
        "AWS::Lambda::Permission" => &[
            "Action",
            "EventSourceToken",
            "FunctionName",
            "FunctionUrlAuthType",
            "Principal",
            "PrincipalOrgID",
            "SourceAccount",
            "SourceArn",
        ],
        "AWS::Lambda::Version" => &[
            "FunctionName",
            "CodeSha256",
            "Description",
            "ProvisionedConcurrencyConfig",
            "RuntimePolicy",
        ],
        "AWS::Lambda::Alias" => &["FunctionName", "Name"],
        "AWS::Lambda::EventSourceMapping" => &[
            "EventSourceArn",
            "StartingPosition",
            "StartingPositionTimestamp",
            "SelfManagedEventSource",
        ],
        "AWS::Lambda::Url" => &["TargetFunctionArn", "Qualifier"],
        "AWS::Lambda::LayerVersion" => &[
            "CompatibleRuntimes",
            "Content",
            "Description",
            "LayerName",
            "LicenseInfo",
            "CompatibleArchitectures",
        ],
        "AWS::Lambda::LayerVersionPermission" => {
            &["Action", "LayerVersionArn", "OrganizationId", "Principal"]
        }
        "AWS::Lambda::EventInvokeConfig" => &["FunctionName", "Qualifier"],
        _ => create_only_rest(ty),
    }
}

/// [`create_only`] for the types after Lambda.
fn create_only_rest(ty: &str) -> &'static [&'static str] {
    match ty {
        "AWS::Kinesis::Stream" => &["Name"],
        "AWS::Kinesis::StreamConsumer" => &["ConsumerName", "StreamARN"],
        "AWS::KMS::Key" => &["KeySpec", "KeyUsage", "MultiRegion", "Origin"],
        "AWS::KMS::Alias" => &["AliasName"],
        "AWS::SecretsManager::Secret" => &["Name"],
        "AWS::SecretsManager::ResourcePolicy"
        | "AWS::SecretsManager::RotationSchedule"
        | "AWS::SecretsManager::SecretTargetAttachment" => &["SecretId"],
        "AWS::Events::Rule" => &["Name", "EventBusName"],
        "AWS::Events::EventBus" => &["Name", "EventSourceName"],
        "AWS::Events::Archive" => &["ArchiveName", "SourceArn"],
        "AWS::Events::Connection" | "AWS::Events::ApiDestination" => &["Name"],
        "AWS::Events::EventBusPolicy" => &["StatementId", "EventBusName"],
        "AWS::Scheduler::Schedule" => &["Name", "GroupName"],
        "AWS::Scheduler::ScheduleGroup" => &["Name"],
        "AWS::CloudWatch::Alarm" | "AWS::CloudWatch::CompositeAlarm" => &["AlarmName"],
        "AWS::CloudWatch::Dashboard" => &["DashboardName"],
        "AWS::StepFunctions::StateMachine" => &["StateMachineName", "StateMachineType"],
        "AWS::StepFunctions::Activity" => &["Name"],
        "AWS::StepFunctions::StateMachineVersion" => &["StateMachineArn"],
        "AWS::StepFunctions::StateMachineAlias" => &["Name"],
        "AWS::ApiGateway::Resource" => &["ParentId", "PathPart", "RestApiId"],
        "AWS::ApiGateway::Method" => &["HttpMethod", "ResourceId", "RestApiId"],
        "AWS::ApiGateway::Deployment" => &["DeploymentCanarySettings", "RestApiId"],
        "AWS::ApiGateway::Stage" => &["RestApiId", "StageName"],
        "AWS::ApiGateway::Authorizer"
        | "AWS::ApiGateway::Model"
        | "AWS::ApiGateway::RequestValidator" => &["RestApiId", "Name"],
        "AWS::ApiGateway::GatewayResponse" => &["ResponseType", "RestApiId"],
        "AWS::ApiGateway::ApiKey" => &["GenerateDistinctId", "Name", "Value"],
        "AWS::ApiGateway::UsagePlanKey" => &["KeyId", "KeyType", "UsagePlanId"],
        "AWS::ApiGatewayV2::Api" => &["ProtocolType"],
        "AWS::ApiGatewayV2::Route"
        | "AWS::ApiGatewayV2::Integration"
        | "AWS::ApiGatewayV2::Deployment"
        | "AWS::ApiGatewayV2::Authorizer" => &["ApiId"],
        "AWS::ApiGatewayV2::Stage" => &["ApiId", "StageName"],
        _ => &[],
    }
}

/// The property that names a resource of `ty`, when the template may give one (a resource named this way cannot be replaced
/// without a new name).
pub(crate) fn name_property(ty: &str) -> Option<&'static str> {
    Some(match ty {
        "AWS::SQS::Queue" => "QueueName",
        "AWS::SNS::Topic" => "TopicName",
        "AWS::SSM::Parameter"
        | "AWS::SecretsManager::Secret"
        | "AWS::Kinesis::Stream"
        | "AWS::Events::Rule"
        | "AWS::Events::EventBus"
        | "AWS::Scheduler::Schedule"
        | "AWS::Scheduler::ScheduleGroup" => "Name",
        "AWS::S3::Bucket" => "BucketName",
        "AWS::DynamoDB::Table" | "AWS::DynamoDB::GlobalTable" => "TableName",
        "AWS::IAM::Role" => "RoleName",
        "AWS::IAM::User" => "UserName",
        "AWS::IAM::Group" => "GroupName",
        "AWS::IAM::ManagedPolicy" => "ManagedPolicyName",
        "AWS::IAM::InstanceProfile" => "InstanceProfileName",
        "AWS::Logs::LogGroup" => "LogGroupName",
        "AWS::Lambda::Function" => "FunctionName",
        "AWS::KMS::Alias" => "AliasName",
        "AWS::CloudWatch::Alarm" | "AWS::CloudWatch::CompositeAlarm" => "AlarmName",
        "AWS::StepFunctions::StateMachine" => "StateMachineName",
        _ => return None,
    })
}

/// The primary identifier of `ty` (for `GetTemplateSummary`).
pub(crate) fn identifiers(ty: &str) -> Option<Vec<&'static str>> {
    Some(vec![match ty {
        "AWS::SQS::Queue" => "QueueUrl",
        "AWS::SNS::Topic" => "TopicArn",
        "AWS::SSM::Parameter" => "Name",
        "AWS::S3::Bucket" | "AWS::S3::BucketPolicy" => "BucketName",
        "AWS::DynamoDB::Table" | "AWS::DynamoDB::GlobalTable" => "TableName",
        "AWS::IAM::Role" => "RoleName",
        "AWS::IAM::User" => "UserName",
        "AWS::IAM::Group" => "GroupName",
        "AWS::Logs::LogGroup" => "LogGroupName",
        "AWS::Lambda::Function" => "FunctionName",
        "AWS::Kinesis::Stream" => "Name",
        "AWS::KMS::Key" => "KeyId",
        "AWS::SecretsManager::Secret" => "Id",
        "AWS::Events::Rule" => "Arn",
        "AWS::Scheduler::Schedule" | "AWS::Scheduler::ScheduleGroup" => "Name",
        "AWS::StepFunctions::StateMachine" => "Arn",
        _ => return None,
    }])
}
