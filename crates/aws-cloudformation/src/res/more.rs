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
    "AWS::CloudFormation::Stack",
    "AWS::CloudFormation::CustomResource",
    "AWS::StepFunctions::StateMachine",
    "AWS::StepFunctions::Activity",
    "AWS::StepFunctions::StateMachineVersion",
    "AWS::StepFunctions::StateMachineAlias",
    "AWS::ApiGateway::RestApi",
    "AWS::ApiGateway::Resource",
    "AWS::ApiGateway::Method",
    "AWS::ApiGateway::Deployment",
    "AWS::ApiGateway::Stage",
    "AWS::ApiGateway::Authorizer",
    "AWS::ApiGateway::ApiKey",
    "AWS::ApiGateway::UsagePlan",
    "AWS::ApiGateway::UsagePlanKey",
    "AWS::ApiGateway::Model",
    "AWS::ApiGateway::RequestValidator",
    "AWS::ApiGateway::GatewayResponse",
    "AWS::ApiGatewayV2::Api",
    "AWS::ApiGatewayV2::Route",
    "AWS::ApiGatewayV2::Integration",
    "AWS::ApiGatewayV2::Stage",
    "AWS::ApiGatewayV2::Deployment",
    "AWS::ApiGatewayV2::Authorizer",
    "AWS::ApiGatewayV2::RouteResponse",
    "AWS::ApiGatewayV2::IntegrationResponse",
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

pub(super) async fn create(rc: &Rc, p: &Value) -> Done {
    match rc.ty.as_str() {
        "AWS::CloudFormation::Stack" => crate::nested::create(rc, p).await,
        t if crate::custom::is_custom(t) => crate::custom::create(rc, p).await,
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
    if rc.ty == "AWS::CloudFormation::Stack" {
        return crate::nested::update(rc, old, p).await;
    }
    if crate::custom::is_custom(&rc.ty) {
        return crate::custom::update(rc, old, p).await;
    }
    if natively_made(&rc.ty) {
        let mut m = Made::new(old.physical.clone().unwrap_or_default());
        m.attrs = old.attrs_map();
        return Ok(m);
    }
    create(rc, p).await
}

pub(super) async fn delete(rc: &Rc, res: &Res) -> Result<(), String> {
    if rc.ty == "AWS::CloudFormation::Stack" {
        return crate::nested::delete(rc, res).await;
    }
    if crate::custom::is_custom(&rc.ty) {
        return crate::custom::delete(rc, res).await;
    }
    Ok(())
}
