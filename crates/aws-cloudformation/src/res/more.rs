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
    "AWS::SSM::Document",
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
    "AWS::Scheduler::Schedule",
    "AWS::Scheduler::ScheduleGroup",
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

/// The mapped type `ty` is probably a misspelling of: the same family (or a family one letter away) and a type name one letter away
/// (two for a name of five letters or more), or the same but for case. `None` for a type that is not near one.
pub(crate) fn near_miss(ty: &str) -> Option<&'static str> {
    let (family, name) = split(ty)?;
    MAPPED.iter().copied().find(|m| {
        let Some((mf, mn)) = split(m) else {
            return false;
        };
        let family_near = family == mf || (mf.len() > 2 && distance(family, mf) <= 1);
        let name_near = name.eq_ignore_ascii_case(mn)
            || distance(name, mn) <= if mn.len() >= 5 { 2 } else { 1 };
        family_near && name_near && ty != *m
    })
}

/// The family and the type name of `AWS::<family>::<name>`.
fn split(ty: &str) -> Option<(&str, &str)> {
    let rest = ty.strip_prefix("AWS::")?;
    rest.split_once("::")
}

/// The edit distance between two names (letters inserted, removed or changed).
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut prev = row.first().copied().unwrap_or(0);
        if let Some(first) = row.first_mut() {
            *first = i + 1;
        }
        for (j, cb) in b.iter().enumerate() {
            let above = row.get(j + 1).copied().unwrap_or(usize::MAX);
            let left = row.get(j).copied().unwrap_or(usize::MAX);
            let cost = prev + usize::from(ca != *cb);
            prev = above;
            if let Some(cell) = row.get_mut(j + 1) {
                *cell = cost.min(above + 1).min(left + 1);
            }
        }
    }
    row.last().copied().unwrap_or(0)
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
        "AWS::CloudFormation::WaitConditionHandle" => Ok(Made::new(crate::signal::handle_url(
            &rc.caller.endpoint,
            &rc.caller.region,
            &rc.stack_id,
            &rc.logical,
        ))),
        // Made with its creation policy in `engine::launch`; without one, from its properties.
        "AWS::CloudFormation::WaitCondition" => super::wait::create(rc, p, None).await,
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
    if rc.ty == "AWS::CloudFormation::WaitConditionHandle"
        && let Some(url) = &res.physical
    {
        crate::signal::close_handle(url);
    }
    Ok(())
}
