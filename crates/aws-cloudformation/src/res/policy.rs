//! The class of every resource type (decision D5): **mapped** (made locally), **skipped** (opaque infrastructure with no behaviour an
//! application can observe: accepted, given an identity, reported as not made), or **refused** (something an application would
//! connect to, refused before anything is made, naming the resource and why). `Config` turns the default either way, for all types
//! or one.

use serde_json::Value;

use aws_wire::ApiError;

use crate::errors::{format, template};
use crate::{Config, Unsupported};

/// What happens to a resource of a type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Class {
    /// Made locally through its service.
    Map,
    /// Accepted unmade; the reason goes on the resource.
    Skip(String),
    /// Refused before anything is made.
    Fail(String),
    /// Not a type CloudFormation has.
    Unknown,
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

/// Types of a served family that are skipped (stored nowhere, reported on the resource), with why.
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
    (
        "AWS::SSM::Document",
        "SSM documents (Parameter Store is what doze-aws serves of SSM)",
    ),
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

/// Families skipped whole: opaque infrastructure and governance (SCOPE.md section 3.1).
const SKIPPED_FAMILIES: &[&str] = &[
    "ApplicationAutoScaling",
    "CloudFront",
    "Route53",
    "Route53Resolver",
    "CertificateManager",
    "WAF",
    "WAFv2",
    "WAFRegional",
    "XRay",
    "Config",
    "CloudTrail",
    "GuardDuty",
    "Backup",
    "SecurityHub",
    "AccessAnalyzer",
    "Inspector",
    "InspectorV2",
    "Macie",
    "Detective",
    "Budgets",
    "CE",
    "ResourceGroups",
    "ServiceCatalog",
    "SSO",
    "Signer",
    "ECR",
    "Shield",
    "FMS",
    "NetworkFirewall",
    "Organizations",
    "ResourceExplorer2",
    "Notifications",
    "CodeStarNotifications",
    "Synthetics",
    "RUM",
    "Evidently",
    "ApplicationInsights",
];

/// Types refused within a family that is otherwise skipped (EC2 compute, RDS databases).
fn refused_within(ty: &str) -> bool {
    matches!(
        ty,
        "AWS::EC2::Instance"
            | "AWS::EC2::SpotFleet"
            | "AWS::EC2::EC2Fleet"
            | "AWS::EC2::Host"
            | "AWS::EC2::CapacityReservation"
            | "AWS::EC2::Volume"
            | "AWS::EC2::VolumeAttachment"
    ) || ty.starts_with("AWS::RDS::DB")
        && !ty.ends_with("SubnetGroup")
        && !ty.ends_with("ParameterGroup")
        && !ty.ends_with("SecurityGroup")
        || ty == "AWS::RDS::GlobalCluster"
        || ty == "AWS::RDS::Integration"
}

/// The class of `ty` before configuration.
fn default_class(ty: &str) -> Class {
    let skip = |what: &str| {
        Class::Skip(format!(
            "Not created locally by doze-aws: stored, not active ({what} have no local counterpart)"
        ))
    };
    if ty.starts_with("Custom::") || crate::res::more::natively_made(ty) {
        return Class::Map;
    }
    let mut parts = ty.split("::");
    let (Some(ns), Some(family), Some(_), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Class::Unknown;
    };
    if ns != "AWS" && ns != "Alexa" {
        return Class::Unknown;
    }
    if let Some((_, what)) = SKIPPED.iter().find(|(t, _)| *t == ty) {
        return skip(what);
    }
    if family == "Serverless" {
        return Class::Fail(format!(
            "{ty} must be expanded by the AWS::Serverless-2016-10-31 transform; the template does not declare it"
        ));
    }
    if SERVED.contains(&family) {
        return if crate::res::more::mapped(ty) {
            Class::Map
        } else {
            Class::Fail(format!(
                "{ty} is unknown to this version of doze-aws ({family} itself is served)"
            ))
        };
    }
    if refused_within(ty) {
        return Class::Fail(format!(
            "doze-aws does not serve {ty}: application code would connect to it"
        ));
    }
    if family == "EC2" || family == "RDS" || SKIPPED_FAMILIES.contains(&family) {
        return skip(&format!("{family} resources"));
    }
    Class::Fail(format!("doze-aws does not serve {family}"))
}

/// The class of `ty` under `config`.
pub(crate) fn class(ty: &str, config: &Config) -> Class {
    let base = default_class(ty);
    let chosen = config
        .overrides
        .iter()
        .find(|(t, _)| t == ty)
        .map_or(config.unsupported, |(_, u)| *u);
    match (base, chosen) {
        (Class::Fail(why), Unsupported::Skip) | (Class::Skip(why), Unsupported::Skip) => {
            Class::Skip(format!(
                "Not created locally by doze-aws: stored, not active ({why}; skipped by configuration)"
            ))
        }
        (Class::Skip(why), Unsupported::Fail) => {
            Class::Fail(format!("{why}; refused by configuration"))
        }
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
    let mut refused = Vec::new();
    for (logical, r) in res {
        let ty = crate::tmpl::type_of(r);
        match class(ty, config) {
            Class::Unknown => unknown.push(ty.to_owned()),
            Class::Fail(why) => refused.push(format!("{logical} ({ty}): {why}")),
            Class::Map | Class::Skip(_) => {}
        }
    }
    if !unknown.is_empty() {
        unknown.sort();
        unknown.dedup();
        // (memory)
        return Err(format(format!(
            "Unrecognized resource types: [{}]",
            unknown.join(", ")
        )));
    }
    if !refused.is_empty() {
        return Err(template(format!(
            "doze-aws refuses {}. Start doze-aws with --cfn-unsupported skip (or --cfn-skip <type>) to accept them unmade",
            refused.join("; ")
        )));
    }
    Ok(())
}
