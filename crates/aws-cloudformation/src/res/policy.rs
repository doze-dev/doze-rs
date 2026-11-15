//! The class of every resource type: **mapped** (made locally), **skipped** (accepted without being made: the resource reaches
//! `CREATE_COMPLETE` with a status reason that says so, `Ref` and `Fn::GetAtt` give stable placeholders, and the deploy logs one
//! warning per type), or **refused** before anything is made.
//!
//! A deploy never fails for a type of a service doze-aws does not run: an EC2 instance, an RDS database, a Cognito pool, an ECS
//! service are skipped, whatever they are, because some of those services will never run here and a template that has them must
//! still deploy what doze-aws does run. Strict mode (`--cfn-unsupported fail`) refuses every type that would be skipped, for
//! people who want to know a template is not wholly local.
//!
//! Refused whatever the mode, as AWS refuses them: a type CloudFormation does not have (`Unrecognized resource types`, recorded as
//! a `ValidationError` of template format), which here includes a type that is one or two letters from a type doze-aws makes
//! (`AWS::Lambda::Funtion`, `AWS::Lamda::Function`); and a `AWS::Serverless::*` type without the transform. A near miss is refused
//! rather than skipped because skipping it would deploy a stack without the function or queue the developer meant, and the failure
//! would show later, far from its cause; the refusal names the type it is probably meant to be. A type of a family doze-aws runs that
//! is not near one it makes is a real type not built yet (`AWS::SSM::Document`): it is skipped like any other.

use serde_json::Value;

use aws_wire::ApiError;

use crate::errors::{format, template};
use crate::{Config, Unsupported};

/// What happens to a resource of a type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Class {
    /// Made locally through its service.
    Map,
    /// Accepted unmade, for this cause (`doze-aws does not run EC2`); [`reason`] is what goes on the resource.
    Skip(String),
    /// Refused before anything is made.
    Fail(String),
    /// Not a type CloudFormation has; the type it is probably meant to be, when one is near.
    Unknown(Option<&'static str>),
}

/// The families whose types are made (some types of a family are skipped below).
const SERVED: &[&str] = &[
    "SQS",
    "SNS",
    "SSM",
    "S3",
    "DynamoDB",
    "IAM",
    "Logs",
    "Lambda",
    "Kinesis",
    "KMS",
    "SecretsManager",
    "Events",
    "CloudWatch",
    "StepFunctions",
    "ApiGateway",
    "ApiGatewayV2",
    "CloudFormation",
    "Scheduler",
];

/// Types of a served family that are skipped (stored nowhere, reported on the resource), with what doze-aws does not run of it.
const SKIPPED: &[(&str, &str)] = &[
    ("AWS::SSM::Association", "SSM associations"),
    ("AWS::SSM::MaintenanceWindow", "SSM maintenance windows"),
    (
        "AWS::SSM::MaintenanceWindowTarget",
        "SSM maintenance windows",
    ),
    ("AWS::SSM::MaintenanceWindowTask", "SSM maintenance windows"),
    ("AWS::SSM::PatchBaseline", "SSM patch baselines"),
    ("AWS::SSM::ResourceDataSync", "SSM resource data syncs"),
    ("AWS::ApiGateway::DomainName", "custom domain names"),
    ("AWS::ApiGateway::DomainNameV2", "custom domain names"),
    ("AWS::ApiGateway::BasePathMapping", "custom domain names"),
    ("AWS::ApiGateway::BasePathMappingV2", "custom domain names"),
    ("AWS::ApiGateway::VpcLink", "VPC links"),
    ("AWS::ApiGateway::ClientCertificate", "client certificates"),
    ("AWS::ApiGateway::DocumentationPart", "API documentation"),
    ("AWS::ApiGateway::DocumentationVersion", "API documentation"),
    (
        "AWS::ApiGateway::Account",
        "the API Gateway account's logging role",
    ),
    ("AWS::ApiGatewayV2::DomainName", "custom domain names"),
    ("AWS::ApiGatewayV2::ApiMapping", "custom domain names"),
    ("AWS::ApiGatewayV2::VpcLink", "VPC links"),
    ("AWS::Lambda::CodeSigningConfig", "code signing"),
    ("AWS::CloudWatch::AnomalyDetector", "anomaly detection"),
    ("AWS::CloudWatch::InsightRule", "Contributor Insights"),
    ("AWS::CloudWatch::MetricStream", "metric streams"),
    ("AWS::SecretsManager::RotationSchedule", "secret rotation"),
    (
        "AWS::SecretsManager::SecretTargetAttachment",
        "secret target attachments",
    ),
    ("AWS::KMS::ReplicaKey", "multi-region replica keys"),
    ("AWS::Events::Endpoint", "global endpoints"),
    ("AWS::Kinesis::ResourcePolicy", "Kinesis resource policies"),
    ("AWS::Logs::Destination", "cross-account log destinations"),
    ("AWS::Logs::Delivery", "log deliveries"),
    ("AWS::Logs::DeliveryDestination", "log deliveries"),
    ("AWS::Logs::DeliverySource", "log deliveries"),
    ("AWS::Logs::AccountPolicy", "account-level log policies"),
    ("AWS::S3::AccessPoint", "S3 access points"),
    ("AWS::S3::MultiRegionAccessPoint", "S3 access points"),
    ("AWS::S3::StorageLens", "S3 Storage Lens"),
    (
        "AWS::CloudFormation::ResourceDefaultVersion",
        "the extension registry",
    ),
    (
        "AWS::CloudFormation::ResourceVersion",
        "the extension registry",
    ),
    (
        "AWS::CloudFormation::TypeActivation",
        "the extension registry",
    ),
    (
        "AWS::CloudFormation::PublicTypeVersion",
        "the extension registry",
    ),
    ("AWS::CloudFormation::Publisher", "the extension registry"),
    (
        "AWS::CloudFormation::HookDefaultVersion",
        "the extension registry",
    ),
    (
        "AWS::CloudFormation::HookTypeConfig",
        "the extension registry",
    ),
];

/// The status reason of a resource skipped for `cause`: plainly, that it was not made and why.
pub(crate) fn reason(cause: &str) -> String {
    format!(
        "Not created: {cause}. CloudFormation accepted the resource without making it; Ref and Fn::GetAtt give placeholders."
    )
}

fn skipped(cause: &str) -> Class {
    Class::Skip(cause.to_owned())
}

/// The class of `ty` before configuration.
fn default_class(ty: &str) -> Class {
    if ty.starts_with("Custom::") || crate::res::more::mapped(ty) {
        return Class::Map;
    }
    let mut parts = ty.split("::");
    let (Some(ns), Some(family), Some(_), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Class::Unknown(None);
    };
    if ns != "AWS" && ns != "Alexa" {
        return Class::Unknown(None);
    }
    if let Some((_, what)) = SKIPPED.iter().find(|(t, _)| *t == ty) {
        return skipped(&format!("doze-aws runs {family} but not {what}"));
    }
    if family == "Serverless" {
        return Class::Fail(format!(
            "{ty} must be expanded by the AWS::Serverless-2016-10-31 transform; the template does not declare it"
        ));
    }
    if let Some(near) = crate::res::more::near_miss(ty) {
        return Class::Unknown(Some(near));
    }
    if SERVED.contains(&family) {
        return skipped(&format!(
            "doze-aws runs {family} but does not make {ty} yet"
        ));
    }
    skipped(&format!("doze-aws does not run {family}"))
}

/// The class of `ty` under `config`: a per-type setting first, then the mode.
pub(crate) fn class(ty: &str, config: &Config) -> Class {
    let base = default_class(ty);
    let chosen = config
        .overrides
        .iter()
        .find(|(t, _)| t == ty)
        .map(|(_, u)| *u);
    match (base, chosen.unwrap_or(config.unsupported)) {
        // A near miss is skipped only when the type is named on its own (`--cfn-skip`), never by the mode.
        (Class::Unknown(Some(_)) | Class::Fail(_), Unsupported::Skip) if chosen.is_some() => {
            skipped(&format!("{ty} is skipped by --cfn-skip"))
        }
        (Class::Skip(cause), Unsupported::Fail) => Class::Fail(cause),
        (other, _) => other,
    }
}

/// Refuses a template with types that are unknown or refused, before anything is made.
///
/// # Errors
/// `ValidationError` naming every such resource and why.
pub(crate) fn check(t: &Value, config: &Config) -> Result<(), ApiError> {
    let Some(res) = crate::tmpl::resources(t) else {
        return Ok(());
    };
    let mut unknown = Vec::new();
    let mut hints = Vec::new();
    let mut refused = Vec::new();
    for (logical, r) in res {
        let ty = crate::tmpl::type_of(r);
        match class(ty, config) {
            Class::Unknown(near) => {
                unknown.push(ty.to_owned());
                if let Some(near) = near {
                    hints.push(format!("{ty} is probably {near}"));
                }
            }
            Class::Fail(why) => refused.push(format!("{logical} ({ty}): {why}")),
            Class::Map | Class::Skip(_) => {}
        }
    }
    if !unknown.is_empty() {
        unknown.sort();
        unknown.dedup();
        hints.sort();
        hints.dedup();
        // (memory) AWS's words; what follows them is doze-aws's.
        let mut message = format!("Unrecognized resource types: [{}]", unknown.join(", "));
        if !hints.is_empty() {
            message.push_str(&format!(" ({})", hints.join("; ")));
        }
        return Err(format(message));
    }
    if !refused.is_empty() {
        let strict = config.unsupported == Unsupported::Fail;
        return Err(template(format!(
            "doze-aws refuses {}.{}",
            refused.join("; "),
            if strict {
                " Strict mode (--cfn-unsupported fail) is on: start doze-aws without it, or with --cfn-skip <type>, to accept them unmade"
            } else {
                ""
            }
        )));
    }
    Ok(())
}
