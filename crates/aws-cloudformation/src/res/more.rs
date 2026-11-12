//! The families beyond the first six, the types CloudFormation makes itself (`AWS::CDK::Metadata`, wait condition handles), and the
//! list of mapped types.

use serde_json::Value;

use super::{Done, Made, Rc};
use crate::model::Res;

/// The mapped types of the served families.
const MAPPED: &[&str] = &[
    "AWS::SQS::Queue",
    "AWS::SQS::QueuePolicy",
    "AWS::SQS::QueueInlinePolicy",
    "AWS::SNS::Topic",
    "AWS::SNS::Subscription",
    "AWS::SNS::TopicPolicy",
    "AWS::SNS::TopicInlinePolicy",
    "AWS::SSM::Parameter",
    "AWS::S3::Bucket",
    "AWS::S3::BucketPolicy",
    "AWS::DynamoDB::Table",
    "AWS::DynamoDB::GlobalTable",
    "AWS::IAM::Role",
    "AWS::IAM::User",
    "AWS::IAM::Group",
    "AWS::IAM::Policy",
    "AWS::IAM::RolePolicy",
    "AWS::IAM::UserPolicy",
    "AWS::IAM::GroupPolicy",
    "AWS::IAM::ManagedPolicy",
    "AWS::IAM::AccessKey",
    "AWS::IAM::UserToGroupAddition",
    "AWS::IAM::InstanceProfile",
    "AWS::IAM::ServiceLinkedRole",
    "AWS::IAM::OIDCProvider",
    "AWS::IAM::SAMLProvider",
    "AWS::Logs::LogGroup",
    "AWS::Logs::LogStream",
    "AWS::Logs::MetricFilter",
    "AWS::Logs::SubscriptionFilter",
    "AWS::Logs::ResourcePolicy",
    "AWS::Logs::QueryDefinition",
    "AWS::Lambda::Function",
    "AWS::Lambda::Permission",
    "AWS::Lambda::Version",
    "AWS::Lambda::Alias",
    "AWS::Lambda::EventSourceMapping",
    "AWS::Lambda::Url",
    "AWS::Lambda::LayerVersion",
    "AWS::Lambda::LayerVersionPermission",
    "AWS::Lambda::EventInvokeConfig",
    "AWS::Kinesis::Stream",
    "AWS::Kinesis::StreamConsumer",
    "AWS::KMS::Key",
    "AWS::KMS::Alias",
    "AWS::SecretsManager::Secret",
    "AWS::SecretsManager::ResourcePolicy",
    "AWS::Events::Rule",
    "AWS::Events::EventBus",
    "AWS::Events::EventBusPolicy",
    "AWS::Events::Archive",
    "AWS::Events::Connection",
    "AWS::Events::ApiDestination",
    "AWS::CloudWatch::Alarm",
    "AWS::CloudWatch::CompositeAlarm",
    "AWS::CloudWatch::Dashboard",
];

/// Types CloudFormation itself makes: nothing to call.
pub(crate) fn natively_made(ty: &str) -> bool {
    matches!(
        ty,
        "AWS::CDK::Metadata"
            | "AWS::CloudFormation::WaitConditionHandle"
            | "AWS::CloudFormation::WaitCondition"
    )
}

/// Whether `ty` is a mapped type.
pub(crate) fn mapped(ty: &str) -> bool {
    MAPPED.contains(&ty) || natively_made(ty)
}

pub(super) async fn create(rc: &Rc, _p: &Value) -> Done {
    match rc.ty.as_str() {
        "AWS::CDK::Metadata" => Ok(Made::new(aws_seal::uuid_v4().unwrap_or_default())),
        "AWS::CloudFormation::WaitConditionHandle" => {
            let (url, _rx) = rc.runner.responses.expect(
                &rc.caller.endpoint,
                &rc.caller.region,
                &rc.stack_id,
                &rc.logical,
            );
            Ok(Made::new(url))
        }
        "AWS::CloudFormation::WaitCondition" => {
            // (memory) With nothing local to send a signal, a wait condition completes at once and its Data is empty.
            Ok(Made::new(super::generated_name(rc, 128, false)).with("Data", "{}"))
        }
        other => Err(format!("{other} is not a type doze-aws makes")),
    }
}

pub(super) async fn update(rc: &Rc, old: &Res, p: &Value) -> Done {
    if natively_made(&rc.ty) {
        let mut m = Made::new(old.physical.clone().unwrap_or_default());
        m.attrs = old.attrs_map();
        return Ok(m);
    }
    create(rc, p).await
}

pub(super) async fn delete(_rc: &Rc, _res: &Res) -> Result<(), String> {
    Ok(())
}
