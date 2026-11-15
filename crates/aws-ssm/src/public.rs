//! Public parameters: the read-only names AWS publishes under `/aws/service/` for every account, which templates and CDK resolve
//! to find an image (`MachineImage.latestAmazonLinux2023()` reads `/aws/service/ami-amazon-linux-latest/al2023-ami-kernel-default-x86_64`,
//! a template parameter of type `AWS::SSM::Parameter::Value<AWS::EC2::Image::Id>` reads whatever name it is given).
//!
//! A small table of the names in common use is served: the Amazon Linux 2 and 2023 images by kernel and architecture, the ECS- and
//! EKS-optimized images, Bottlerocket, Ubuntu and Windows Server, and the `global-infrastructure` regions and services. The image
//! ids are PLACEHOLDERS shaped like AWS's (`ami-0` and 16 hex digits, stable per region and name): they are not real images and
//! launch nothing; they are there so a template or a CDK app that looks an image up deploys. Nothing is stored: a name is made
//! when it is asked for, and the table costs no memory while no one asks.
//!
//! As on AWS: a public parameter's ARN has no account (`arn:aws:ssm:<region>::parameter/aws/service/...`), `GetParametersByPath`
//! lists them (one level or recursively), `DescribeParameters` does not (it lists the account's own, the model's documentation
//! says), and a write under `/aws/` is refused (`name::check_write`).

/// Where public parameters live.
pub(crate) const ROOT: &str = "/aws/service/";

/// The version and the modification time every public parameter reports (fixed: 2026-01-01T00:00:00Z).
pub(crate) const VERSION: i64 = 1;
pub(crate) const MODIFIED_MS: i64 = 1_767_225_600_000;

/// A public parameter: its name and value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Public {
    pub(crate) name: String,
    pub(crate) value: String,
}

const AL: &str = "/aws/service/ami-amazon-linux-latest/";
const AMAZON_LINUX: &[&str] = &[
    "al2023-ami-kernel-default-x86_64",
    "al2023-ami-kernel-default-arm64",
    "al2023-ami-kernel-6.1-x86_64",
    "al2023-ami-kernel-6.1-arm64",
    "al2023-ami-kernel-6.12-x86_64",
    "al2023-ami-kernel-6.12-arm64",
    "al2023-ami-minimal-kernel-default-x86_64",
    "al2023-ami-minimal-kernel-default-arm64",
    "al2023-ami-minimal-kernel-6.1-x86_64",
    "al2023-ami-minimal-kernel-6.1-arm64",
    "amzn2-ami-hvm-x86_64-gp2",
    "amzn2-ami-hvm-arm64-gp2",
    "amzn2-ami-hvm-x86_64-ebs",
    "amzn2-ami-kernel-5.10-hvm-x86_64-gp2",
    "amzn2-ami-kernel-5.10-hvm-arm64-gp2",
    "amzn2-ami-kernel-5.10-hvm-x86_64-ebs",
    "amzn2-ami-minimal-hvm-x86_64-ebs",
    "amzn2-ami-minimal-hvm-arm64-ebs",
];

const ECS: &str = "/aws/service/ecs/optimized-ami/";
/// ECS-optimized images: the path under [`ECS`], the OS it names, and the architecture of its image name.
const ECS_IMAGES: &[(&str, &str, &str)] = &[
    ("amazon-linux-2/recommended", "Amazon Linux 2", "x86_64"),
    (
        "amazon-linux-2/arm64/recommended",
        "Amazon Linux 2",
        "arm64",
    ),
    ("amazon-linux-2/gpu/recommended", "Amazon Linux 2", "x86_64"),
    (
        "amazon-linux-2/kernel-5.10/recommended",
        "Amazon Linux 2",
        "x86_64",
    ),
    (
        "amazon-linux-2/kernel-5.10/arm64/recommended",
        "Amazon Linux 2",
        "arm64",
    ),
    (
        "amazon-linux-2023/recommended",
        "Amazon Linux 2023",
        "x86_64",
    ),
    (
        "amazon-linux-2023/arm64/recommended",
        "Amazon Linux 2023",
        "arm64",
    ),
    (
        "amazon-linux-2023/gpu/recommended",
        "Amazon Linux 2023",
        "x86_64",
    ),
];

const BOTTLEROCKET: &[&str] = &["aws-ecs-2", "aws-k8s-1.31", "aws-k8s-1.32", "aws-k8s-1.33"];
const EKS_VERSIONS: &[&str] = &["1.31", "1.32", "1.33"];
const ARCHES: &[&str] = &["x86_64", "arm64"];

/// Ubuntu Server images: release and volume type.
const UBUNTU: &[(&str, &str)] = &[("22.04", "ebs-gp2"), ("24.04", "ebs-gp3")];

const WINDOWS: &[&str] = &[
    "Windows_Server-2019-English-Full-Base",
    "Windows_Server-2022-English-Full-Base",
    "Windows_Server-2025-English-Full-Base",
];

const INFRA: &str = "/aws/service/global-infrastructure/";
/// Region codes and their long names (memory).
const REGIONS: &[(&str, &str)] = &[
    ("af-south-1", "Africa (Cape Town)"),
    ("ap-east-1", "Asia Pacific (Hong Kong)"),
    ("ap-northeast-1", "Asia Pacific (Tokyo)"),
    ("ap-northeast-2", "Asia Pacific (Seoul)"),
    ("ap-northeast-3", "Asia Pacific (Osaka)"),
    ("ap-south-1", "Asia Pacific (Mumbai)"),
    ("ap-south-2", "Asia Pacific (Hyderabad)"),
    ("ap-southeast-1", "Asia Pacific (Singapore)"),
    ("ap-southeast-2", "Asia Pacific (Sydney)"),
    ("ap-southeast-3", "Asia Pacific (Jakarta)"),
    ("ap-southeast-4", "Asia Pacific (Melbourne)"),
    ("ca-central-1", "Canada (Central)"),
    ("ca-west-1", "Canada West (Calgary)"),
    ("eu-central-1", "Europe (Frankfurt)"),
    ("eu-central-2", "Europe (Zurich)"),
    ("eu-north-1", "Europe (Stockholm)"),
    ("eu-south-1", "Europe (Milan)"),
    ("eu-south-2", "Europe (Spain)"),
    ("eu-west-1", "Europe (Ireland)"),
    ("eu-west-2", "Europe (London)"),
    ("eu-west-3", "Europe (Paris)"),
    ("il-central-1", "Israel (Tel Aviv)"),
    ("me-central-1", "Middle East (UAE)"),
    ("me-south-1", "Middle East (Bahrain)"),
    ("sa-east-1", "South America (Sao Paulo)"),
    ("us-east-1", "US East (N. Virginia)"),
    ("us-east-2", "US East (Ohio)"),
    ("us-west-1", "US West (N. California)"),
    ("us-west-2", "US West (Oregon)"),
];
/// Service codes and their long names (memory): the services doze-aws runs, and the compute ones a template names beside them.
const SERVICES: &[(&str, &str)] = &[
    ("apigateway", "Amazon API Gateway"),
    ("cloudformation", "AWS CloudFormation"),
    ("cloudwatch", "Amazon CloudWatch"),
    ("dynamodb", "Amazon DynamoDB"),
    ("ec2", "Amazon Elastic Compute Cloud (EC2)"),
    ("ecs", "Amazon Elastic Container Service (ECS)"),
    ("eks", "Amazon Elastic Kubernetes Service (EKS)"),
    ("events", "Amazon EventBridge"),
    ("iam", "AWS Identity and Access Management (IAM)"),
    ("kinesis", "Amazon Kinesis Data Streams"),
    ("kms", "AWS Key Management Service"),
    ("lambda", "AWS Lambda"),
    ("logs", "Amazon CloudWatch Logs"),
    ("s3", "Amazon Simple Storage Service (S3)"),
    ("scheduler", "Amazon EventBridge Scheduler"),
    ("secretsmanager", "AWS Secrets Manager"),
    ("sns", "Amazon Simple Notification Service (SNS)"),
    ("sqs", "Amazon Simple Queue Service (SQS)"),
    ("ssm", "AWS Systems Manager"),
    ("states", "AWS Step Functions"),
    ("sts", "AWS Security Token Service"),
];

/// The placeholder image id of `name` in `region`: `ami-0` and 16 hex digits of a hash of both, the same on every call.
pub(crate) fn image_id(region: &str, name: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in region.bytes().chain([0]).chain(name.bytes()) {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("ami-0{h:016x}")
}

fn image(region: &str, name: String) -> Public {
    let value = image_id(region, &name);
    Public { name, value }
}

fn text(name: String, value: impl Into<String>) -> Public {
    Public {
        name,
        value: value.into(),
    }
}

/// Every public parameter of `region`, in no order.
fn all(region: &str) -> Vec<Public> {
    let mut out: Vec<Public> = AMAZON_LINUX
        .iter()
        .map(|n| image(region, format!("{AL}{n}")))
        .collect();
    for (path, os, arch) in ECS_IMAGES {
        ecs(region, path, os, arch, &mut out);
    }
    for arch in ARCHES {
        for variant in BOTTLEROCKET {
            let base = format!("/aws/service/bottlerocket/{variant}/{arch}/latest");
            out.push(image(region, format!("{base}/image_id")));
            out.push(text(format!("{base}/image_version"), "1.30.0-0a9f5ba1"));
        }
        for v in EKS_VERSIONS {
            let name = format!(
                "/aws/service/eks/optimized-ami/{v}/amazon-linux-2023/{arch}/standard/recommended/image_id"
            );
            out.push(image(region, name));
        }
    }
    for (release, volume) in UBUNTU {
        for arch in ["amd64", "arm64"] {
            let name = format!(
                "/aws/service/canonical/ubuntu/server/{release}/stable/current/{arch}/hvm/{volume}/ami-id"
            );
            out.push(image(region, name));
        }
    }
    for w in WINDOWS {
        out.push(image(
            region,
            format!("/aws/service/ami-windows-latest/{w}"),
        ));
    }
    infrastructure(&mut out);
    out
}

/// An ECS-optimized image: the JSON summary at `path`, and its `image_id` and `image_name` beneath it (the shape AWS publishes).
fn ecs(region: &str, path: &str, os: &str, arch: &str, out: &mut Vec<Public>) {
    let base = format!("{ECS}{path}");
    let id = image_id(region, &format!("{base}/image_id"));
    let (family, version) = if os == "Amazon Linux 2023" {
        (
            "al2023-ami-ecs-hvm-2023.0.20260101-kernel-6.1",
            "2023.0.20260101",
        )
    } else {
        ("amzn2-ami-ecs-hvm-2.0.20260101", "2.0.20260101")
    };
    let name = format!("{family}-{arch}-ebs");
    let summary = serde_json::json!({
        "ecs_agent_version": "1.97.0",
        "ecs_runtime_version": "Docker version 25.0.8",
        "image_id": id,
        "image_name": name,
        "image_version": version,
        "os": os,
        "schema_version": 1,
        "source_image_name": format!("{family}-{arch}"),
    });
    out.push(text(base.clone(), summary.to_string()));
    out.push(text(format!("{base}/image_id"), id));
    out.push(text(format!("{base}/image_name"), name));
}

fn infrastructure(out: &mut Vec<Public>) {
    for (code, long) in REGIONS {
        let base = format!("{INFRA}regions/{code}");
        out.push(text(format!("{base}/longName"), *long));
        out.push(text(format!("{base}/partition"), "aws"));
        out.push(text(format!("{base}/domain"), "amazonaws.com"));
        out.push(text(base, *code));
    }
    for (code, long) in SERVICES {
        let base = format!("{INFRA}services/{code}");
        out.push(text(format!("{base}/longName"), *long));
        out.push(text(base, *code));
    }
}

/// Whether `name` is under `/aws/service/`, where only AWS's parameters live.
pub(crate) fn is_public(name: &str) -> bool {
    name.starts_with(ROOT)
}

/// The public parameter called `name` in `region`.
pub(crate) fn find(region: &str, name: &str) -> Option<Public> {
    if !is_public(name) {
        return None;
    }
    all(region).into_iter().find(|p| p.name == name)
}

/// The public parameters under `path` (one level down, or any depth), in name order, after `after`.
pub(crate) fn under(region: &str, path: &str, recursive: bool, after: Option<&str>) -> Vec<Public> {
    let mut found: Vec<Public> = all(region)
        .into_iter()
        .filter(|p| crate::list::under(&p.name, path, recursive))
        .filter(|p| after.is_none_or(|a| p.name.as_str() > a))
        .collect();
    found.sort_by(|a, b| a.name.cmp(&b.name));
    found
}

/// The ARN of a public parameter: it belongs to no account.
pub(crate) fn arn(region: &str, name: &str) -> String {
    format!("arn:aws:ssm:{region}::parameter{name}")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]
    use super::*;

    #[test]
    fn the_names_cdk_reads_are_served() {
        for name in [
            "/aws/service/ami-amazon-linux-latest/al2023-ami-kernel-default-x86_64",
            "/aws/service/ami-amazon-linux-latest/al2023-ami-minimal-kernel-default-arm64",
            "/aws/service/ami-amazon-linux-latest/amzn2-ami-kernel-5.10-hvm-x86_64-gp2",
            "/aws/service/ecs/optimized-ami/amazon-linux-2023/recommended/image_id",
            "/aws/service/bottlerocket/aws-ecs-2/arm64/latest/image_id",
        ] {
            let p = find("us-east-1", name).unwrap();
            assert!(
                p.value.starts_with("ami-0") && p.value.len() == 21,
                "{}",
                p.value
            );
        }
        assert_eq!(find("us-east-1", "/aws/service/nothing"), None);
        assert_eq!(find("us-east-1", "/app/x"), None);
    }

    #[test]
    fn an_image_id_is_stable_and_differs_by_region() {
        let name = "/aws/service/ami-amazon-linux-latest/al2023-ami-kernel-default-x86_64";
        assert_eq!(image_id("us-east-1", name), image_id("us-east-1", name));
        assert_ne!(image_id("us-east-1", name), image_id("eu-west-1", name));
    }

    #[test]
    fn the_ecs_summary_names_its_own_image() {
        let base = "/aws/service/ecs/optimized-ami/amazon-linux-2/recommended";
        let summary = find("us-east-1", base).unwrap();
        let id = find("us-east-1", &format!("{base}/image_id")).unwrap();
        let v: serde_json::Value = serde_json::from_str(&summary.value).unwrap();
        assert_eq!(v["image_id"], id.value.as_str());
    }

    #[test]
    fn a_path_lists_one_level_or_all() {
        let regions = under(
            "us-east-1",
            "/aws/service/global-infrastructure/regions",
            false,
            None,
        );
        assert_eq!(regions.len(), REGIONS.len());
        assert_eq!(
            regions[0].name,
            "/aws/service/global-infrastructure/regions/af-south-1"
        );
        let deep = under(
            "us-east-1",
            "/aws/service/global-infrastructure/regions",
            true,
            None,
        );
        assert_eq!(deep.len(), REGIONS.len() * 4);
        let after = under(
            "us-east-1",
            "/aws/service/global-infrastructure/regions",
            false,
            Some(&regions[0].name),
        );
        assert_eq!(after.len(), REGIONS.len() - 1);
    }
}
