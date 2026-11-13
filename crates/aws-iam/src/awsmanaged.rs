//! AWS-managed policies (`arn:aws:iam::aws:policy/...`), made from their names rather than carried: AWS names them by a
//! convention, and the convention gives a document close enough for a local account to allow what an ordinary role needs.
//!
//! | Name | Document |
//! |---|---|
//! | `AdministratorAccess` | everything |
//! | `ReadOnlyAccess` | the read verbs of every service doze-aws serves |
//! | `PowerUserAccess` | everything but `iam`, `organizations` and `account`, and the few IAM reads it keeps |
//! | `Amazon<Svc>FullAccess`, `AWS<Svc>FullAccess`, `<Svc>FullAccess`, `AWS<Svc>_FullAccess`, `<Svc>ReadWrite` | `<prefix>:*` |
//! | `Amazon<Svc>ReadOnlyAccess` (and the same spellings) | the service's read verbs |
//! | `AWSLambda<Source>ExecutionRole` (Basic, SQSQueue, Kinesis, DynamoDB, VPCAccess) | what the poller reads, and writing logs |
//!
//! A path (`service-role/`) is not part of the rule: the name decides. A name the rules do not cover is not a policy
//! (`NoSuchEntity`), so a template that names one fails where it would otherwise be silently allowed nothing.
//!
//! **The documents are approximate** (decision D9: the real ones are to be recorded from the AWS account): the action lists of the
//! Lambda execution roles and of `PowerUserAccess`'s second statement are from memory, and the read-only and full-access documents
//! cover the services doze-aws serves where AWS's list a great many more actions. Nothing here is built until a policy is named.

use serde_json::{Value, json};

/// The read verbs a read-only policy grants, as action patterns.
const READ_VERBS: &[&str] = &["Get*", "List*", "Describe*", "BatchGet*"];

/// What every Lambda execution role grants for writing its logs.
const LAMBDA_LOGS: &[&str] = &[
    "logs:CreateLogGroup",
    "logs:CreateLogStream",
    "logs:PutLogEvents",
];

/// The name AWS embeds in a policy name, and the service prefix of its actions: the services doze-aws serves, and the services
/// their policies name. Compared without case.
const SERVICES: &[(&str, &str)] = &[
    ("S3", "s3"),
    ("SQS", "sqs"),
    ("SNS", "sns"),
    ("DynamoDB", "dynamodb"),
    ("Kinesis", "kinesis"),
    ("Lambda", "lambda"),
    ("SSM", "ssm"),
    ("SecretsManager", "secretsmanager"),
    ("KeyManagementService", "kms"),
    ("KMS", "kms"),
    ("EventBridge", "events"),
    ("CloudWatchEvents", "events"),
    ("CloudWatch", "cloudwatch"),
    ("CloudWatchLogs", "logs"),
    ("IAM", "iam"),
    ("STS", "sts"),
    ("SecurityTokenService", "sts"),
    ("APIGateway", "apigateway"),
    ("CloudFormation", "cloudformation"),
    ("StepFunctions", "states"),
    ("States", "states"),
];

/// Actions a read-only policy grants beyond the read verbs: the reads whose names are not `Get`, `List` or `Describe`. (memory)
const EXTRA_READS: &[(&str, &[&str])] = &[
    ("dynamodb", &["dynamodb:Query", "dynamodb:Scan"]),
    ("sqs", &["sqs:ReceiveMessage"]),
    (
        "logs",
        &["logs:FilterLogEvents", "logs:StartQuery", "logs:StopQuery"],
    ),
    ("kinesis", &["kinesis:SubscribeToShard"]),
];

/// The document of the AWS-managed policy called `name` (its last path segment), or `None` when the rules do not cover it.
pub(crate) fn document(name: &str) -> Option<String> {
    let doc = match name {
        "AdministratorAccess" => allow(&["*"]),
        "ReadOnlyAccess" => {
            let mut prefixes: Vec<&str> = SERVICES.iter().map(|(_, p)| *p).collect();
            prefixes.sort_unstable();
            prefixes.dedup();
            allow(&prefixes.iter().flat_map(|p| reads(p)).collect::<Vec<_>>())
        }
        // (memory) AWS's PowerUserAccess: everything but identity and account administration, and a handful of those back.
        "PowerUserAccess" => json!({"Version": "2012-10-17", "Statement": [
            {"Effect": "Allow", "NotAction": ["iam:*", "organizations:*", "account:*"], "Resource": "*"},
            {"Effect": "Allow", "Action": [
                "iam:CreateServiceLinkedRole", "iam:DeleteServiceLinkedRole", "iam:ListRoles",
                "organizations:DescribeOrganization", "account:GetAccountInformation", "account:ListRegions"
            ], "Resource": "*"}
        ]}),
        // (memory) The role API Gateway writes access logs with; CDK's `RestApi` makes it by default.
        "AmazonAPIGatewayPushToCloudWatchLogs" => allow(&[
            "logs:CreateLogGroup",
            "logs:CreateLogStream",
            "logs:DescribeLogGroups",
            "logs:DescribeLogStreams",
            "logs:PutLogEvents",
            "logs:GetLogEvents",
            "logs:FilterLogEvents",
        ]),
        "AmazonAPIGatewayAdministrator" => allow(&["apigateway:*"]),
        "AmazonAPIGatewayInvokeFullAccess" => {
            allow(&["execute-api:Invoke", "execute-api:ManageConnections"])
        }
        "AWSLambdaRole" => allow(&["lambda:InvokeFunction"]),
        _ => lambda_execution(name).or_else(|| by_service(name))?,
    };
    Some(doc.to_string())
}

/// `AWSLambda<Source>ExecutionRole`: what the poller reads, and the logs. (memory)
fn lambda_execution(name: &str) -> Option<Value> {
    let source = name
        .strip_prefix("AWSLambda")?
        .strip_suffix("ExecutionRole")?;
    let reads: &[&str] = match source {
        "Basic" => &[],
        "SQSQueue" => &[
            "sqs:ReceiveMessage",
            "sqs:DeleteMessage",
            "sqs:GetQueueAttributes",
        ],
        "DynamoDB" => &[
            "dynamodb:DescribeStream",
            "dynamodb:GetRecords",
            "dynamodb:GetShardIterator",
            "dynamodb:ListStreams",
        ],
        "Kinesis" => &[
            "kinesis:DescribeStream",
            "kinesis:DescribeStreamSummary",
            "kinesis:GetRecords",
            "kinesis:GetShardIterator",
            "kinesis:ListShards",
            "kinesis:ListStreams",
            "kinesis:SubscribeToShard",
        ],
        "VPCAccess" => &[
            "ec2:CreateNetworkInterface",
            "ec2:DescribeNetworkInterfaces",
            "ec2:DescribeSubnets",
            "ec2:DeleteNetworkInterface",
            "ec2:AssignPrivateIpAddresses",
            "ec2:UnassignPrivateIpAddresses",
        ],
        _ => return None,
    };
    let actions: Vec<String> = reads
        .iter()
        .chain(LAMBDA_LOGS)
        .map(|a| (*a).to_owned())
        .collect();
    Some(allow(&actions))
}

/// `<vendor><Service>FullAccess`, `...ReadOnlyAccess` and `...ReadWrite`, the vendor being `Amazon`, `AWS` or nothing, with an
/// optional `_` before the service's part (`AWSLambda_FullAccess`).
fn by_service(name: &str) -> Option<Value> {
    for vendor in ["Amazon", "AWS", ""] {
        let Some(rest) = name.strip_prefix(vendor) else {
            continue;
        };
        if let Some(svc) = rest.strip_suffix("FullAccess")
            && let Some(prefix) = service(svc)
        {
            return Some(allow(&[format!("{prefix}:*")]));
        }
        if let Some(svc) = rest.strip_suffix("ReadOnlyAccess")
            && let Some(prefix) = service(svc)
        {
            return Some(allow(&reads(prefix)));
        }
        if let Some(svc) = rest.strip_suffix("ReadWrite")
            && let Some(prefix) = service(svc)
        {
            return Some(allow(&[format!("{prefix}:*")]));
        }
    }
    None
}

/// The action prefix of a service's part of a policy name (`Lambda_` and `lambda` are `Lambda`).
fn service(part: &str) -> Option<&'static str> {
    let part = part.trim_matches(|c| c == '_' || c == '-');
    SERVICES
        .iter()
        .find(|(name, _)| !part.is_empty() && name.eq_ignore_ascii_case(part))
        .map(|(_, prefix)| *prefix)
}

/// The read actions of one service.
fn reads(prefix: &str) -> Vec<String> {
    let mut out: Vec<String> = READ_VERBS.iter().map(|v| format!("{prefix}:{v}")).collect();
    if let Some((_, extra)) = EXTRA_READS.iter().find(|(p, _)| *p == prefix) {
        out.extend(extra.iter().map(|a| (*a).to_owned()));
    }
    out
}

fn allow<S: AsRef<str>>(actions: &[S]) -> Value {
    let actions: Vec<&str> = actions.iter().map(AsRef::as_ref).collect();
    json!({"Version": "2012-10-17", "Statement": [{"Effect": "Allow", "Action": actions, "Resource": "*"}]})
}

/// The policy id of an AWS-managed policy: the same for a name every time (AWS's are fixed), so a policy made, forgotten and made
/// again shows one id. `ANPA` and 17 characters, as IAM's are.
pub(crate) fn stable_id(name: &str) -> String {
    const SET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(name.as_bytes());
    let mut out = String::from("ANPA");
    for b in digest.iter().take(17) {
        let ix = usize::from(b & 31);
        out.push(char::from(SET.get(ix).copied().unwrap_or(b'A')));
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn actions(name: &str) -> Value {
        let doc: Value = serde_json::from_str(&document(name).unwrap()).unwrap();
        doc["Statement"][0]["Action"].clone()
    }

    #[test]
    fn every_document_made_is_one_iam_accepts() {
        for name in [
            "AdministratorAccess",
            "ReadOnlyAccess",
            "PowerUserAccess",
            "AmazonS3FullAccess",
            "AmazonS3ReadOnlyAccess",
            "AmazonSQSFullAccess",
            "AmazonDynamoDBReadOnlyAccess",
            "AWSLambda_FullAccess",
            "AWSLambda_ReadOnlyAccess",
            "AWSLambdaBasicExecutionRole",
            "AWSLambdaSQSQueueExecutionRole",
            "AWSLambdaKinesisExecutionRole",
            "AWSLambdaDynamoDBExecutionRole",
            "AWSLambdaVPCAccessExecutionRole",
            "AWSLambdaRole",
            "SecretsManagerReadWrite",
            "IAMFullAccess",
            "IAMReadOnlyAccess",
            "CloudWatchLogsFullAccess",
            "AWSStepFunctionsFullAccess",
            "AmazonAPIGatewayPushToCloudWatchLogs",
            "AmazonAPIGatewayAdministrator",
            "AmazonAPIGatewayInvokeFullAccess",
        ] {
            let doc = document(name).unwrap_or_else(|| panic!("{name} has no document"));
            aws_iam_policy::check(&doc, aws_iam_policy::Kind::Identity)
                .unwrap_or_else(|e| panic!("{name}: {}", e.0));
        }
    }

    #[test]
    fn the_names_follow_the_convention() {
        assert_eq!(actions("AmazonSQSFullAccess"), json!(["sqs:*"]));
        assert_eq!(actions("AWSLambda_FullAccess"), json!(["lambda:*"]));
        assert_eq!(
            actions("SecretsManagerReadWrite"),
            json!(["secretsmanager:*"])
        );
        assert_eq!(
            actions("AmazonDynamoDBReadOnlyAccess"),
            json!([
                "dynamodb:Get*",
                "dynamodb:List*",
                "dynamodb:Describe*",
                "dynamodb:BatchGet*",
                "dynamodb:Query",
                "dynamodb:Scan"
            ])
        );
        assert_eq!(
            actions("AWSLambdaBasicExecutionRole"),
            json!([
                "logs:CreateLogGroup",
                "logs:CreateLogStream",
                "logs:PutLogEvents"
            ])
        );
        assert_eq!(
            actions("AWSLambdaSQSQueueExecutionRole"),
            json!([
                "sqs:ReceiveMessage",
                "sqs:DeleteMessage",
                "sqs:GetQueueAttributes",
                "logs:CreateLogGroup",
                "logs:CreateLogStream",
                "logs:PutLogEvents"
            ])
        );
    }

    #[test]
    fn a_name_the_rules_do_not_cover_is_not_a_policy() {
        for name in [
            "AmazonMadeUpFullAccess",
            "AWSLambdaNothingExecutionRole",
            "FullAccess",
            "Whatever",
        ] {
            assert!(document(name).is_none(), "{name}");
        }
    }

    #[test]
    fn an_id_is_the_same_for_a_name_and_shaped_as_iams() {
        let id = stable_id("ReadOnlyAccess");
        assert_eq!(id, stable_id("ReadOnlyAccess"));
        assert_ne!(id, stable_id("AdministratorAccess"));
        assert_eq!(id.len(), 21);
        assert!(id.starts_with("ANPA"));
    }
}
