//! Which services the Resources space shows, grouped by what they are for (the design's index), and which of them have a
//! descriptor yet.

use super::Descriptor;

/// One service in the index.
#[derive(Debug)]
pub struct Entry {
    /// Its name in `--services`.
    pub service: &'static str,
    /// The group it is shown in.
    pub group: &'static str,
    /// Its resources, when described.
    pub descriptor: Option<&'static Descriptor>,
    /// Why it lists nothing, when it is not described.
    pub note: &'static str,
}

const NOT_YET: &str =
    "Not described yet: its resources come to the console in the next part of Resources.";

const fn described(service: &'static str, group: &'static str, d: &'static Descriptor) -> Entry {
    Entry {
        service,
        group,
        descriptor: Some(d),
        note: "",
    }
}

const fn not_yet(service: &'static str, group: &'static str) -> Entry {
    Entry {
        service,
        group,
        descriptor: None,
        note: NOT_YET,
    }
}

/// Every service of the build, in the index's order.
pub static ALL: &[Entry] = &[
    described("sqs", "Messaging and events", &super::sqs::SQS),
    described("sns", "Messaging and events", &super::sns::SNS),
    not_yet("eventbridge", "Messaging and events"),
    not_yet("kinesis", "Messaging and events"),
    not_yet("scheduler", "Messaging and events"),
    described("s3", "Storage and data", &super::s3::S3),
    described("dynamodb", "Storage and data", &super::dynamodb::DYNAMODB),
    described("lambda", "Compute and flow", &super::lambda::LAMBDA),
    not_yet("stepfunctions", "Compute and flow"),
    not_yet("apigateway", "Compute and flow"),
    described("ssm", "Config and secrets", &super::ssm::SSM),
    described(
        "secretsmanager",
        "Config and secrets",
        &super::secrets::SECRETS,
    ),
    described("kms", "Config and secrets", &super::kms::KMS),
    not_yet("logs", "Observability"),
    not_yet("cloudwatch", "Observability"),
    not_yet("cloudformation", "Infrastructure"),
    not_yet("iam", "Identity"),
    Entry {
        service: "sts",
        group: "Identity",
        descriptor: None,
        note: "STS holds no resources: it hands out credentials and says who a caller is.",
    },
];

/// The descriptor of `service`, when it has one.
#[must_use]
pub fn descriptor(service: &str) -> Option<&'static Descriptor> {
    ALL.iter()
        .find(|e| e.service == service)
        .and_then(|e| e.descriptor)
}
