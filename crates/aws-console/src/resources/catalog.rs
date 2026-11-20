//! Which services the Resources space shows, grouped by what they are for (the design's index), each with its descriptor
//! (STS alone holds no resources).

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

const fn described(service: &'static str, group: &'static str, d: &'static Descriptor) -> Entry {
    Entry {
        service,
        group,
        descriptor: Some(d),
        note: "",
    }
}

/// Every service of the build, in the index's order.
pub static ALL: &[Entry] = &[
    described("sqs", "Messaging and events", &super::sqs::SQS),
    described("sns", "Messaging and events", &super::sns::SNS),
    described(
        "eventbridge",
        "Messaging and events",
        &super::eventbridge::EVENTBRIDGE,
    ),
    described("kinesis", "Messaging and events", &super::kinesis::KINESIS),
    described(
        "scheduler",
        "Messaging and events",
        &super::scheduler::SCHEDULER,
    ),
    described("s3", "Storage and data", &super::s3::S3),
    described("dynamodb", "Storage and data", &super::dynamodb::DYNAMODB),
    described("lambda", "Compute and flow", &super::lambda::LAMBDA),
    described(
        "stepfunctions",
        "Compute and flow",
        &super::stepfunctions::STEPFUNCTIONS,
    ),
    described(
        "apigateway",
        "Compute and flow",
        &super::apigateway::APIGATEWAY,
    ),
    described("ssm", "Config and secrets", &super::ssm::SSM),
    described(
        "secretsmanager",
        "Config and secrets",
        &super::secrets::SECRETS,
    ),
    described("kms", "Config and secrets", &super::kms::KMS),
    described("logs", "Observability", &super::logs::LOGS),
    described(
        "cloudwatch",
        "Observability",
        &super::cloudwatch::CLOUDWATCH,
    ),
    described(
        "cloudformation",
        "Infrastructure",
        &super::cloudformation::CLOUDFORMATION,
    ),
    described("iam", "Identity", &super::iam::IAM),
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

#[cfg(test)]
mod tests;
