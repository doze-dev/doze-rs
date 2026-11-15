//! SAM's policy templates (`SQSPollerPolicy: {QueueName: !Ref Q}`): the most used ones, written as SAM's policy_templates.json
//! writes them (memory). A template not listed is refused by name rather than guessed: in `--iam enforce`, a wrong guess would refuse
//! or allow what AWS does not.

use serde_json::{Value, json};

use aws_wire::ApiError;

use crate::errors::validation;

/// An argument of a template, as written (a `Ref` stays a `Ref`).
fn arg(args: &Value, key: &str) -> Value {
    args.get(key).cloned().unwrap_or(Value::Null)
}

/// `arn:${AWS::Partition}:<service>:${AWS::Region}:${AWS::AccountId}:<rest>` with `${name}` bound to `value`.
fn arn(rest: &str, name: &str, value: Value) -> Value {
    json!({"Fn::Sub": [format!("arn:${{AWS::Partition}}:{rest}"), {name: value}]})
}

fn statement(actions: &[&str], resource: Value) -> Value {
    json!({"Effect": "Allow", "Action": actions, "Resource": resource})
}

/// The policy document of template `name` with `args`.
///
/// # Errors
/// A template doze-aws does not write.
pub(crate) fn template(name: &str, args: &Value) -> Result<Value, ApiError> {
    let queue = || {
        arn(
            "sqs:${AWS::Region}:${AWS::AccountId}:${queueName}",
            "queueName",
            arg(args, "QueueName"),
        )
    };
    let table = |suffix: &str| {
        arn(
            &format!("dynamodb:${{AWS::Region}}:${{AWS::AccountId}}:table/${{tableName}}{suffix}"),
            "tableName",
            arg(args, "TableName"),
        )
    };
    let bucket = |suffix: &str| {
        arn(
            &format!("s3:::${{bucketName}}{suffix}"),
            "bucketName",
            arg(args, "BucketName"),
        )
    };
    let statements = match name {
        "SQSPollerPolicy" => vec![statement(
            &[
                "sqs:ChangeMessageVisibility",
                "sqs:ChangeMessageVisibilityBatch",
                "sqs:DeleteMessage",
                "sqs:DeleteMessageBatch",
                "sqs:GetQueueAttributes",
                "sqs:ReceiveMessage",
            ],
            queue(),
        )],
        "SQSSendMessagePolicy" => vec![statement(&["sqs:SendMessage*"], queue())],
        "SNSPublishMessagePolicy" => vec![statement(
            &["sns:Publish"],
            arn(
                "sns:${AWS::Region}:${AWS::AccountId}:${topicName}",
                "topicName",
                arg(args, "TopicName"),
            ),
        )],
        "DynamoDBCrudPolicy" => vec![statement(
            &[
                "dynamodb:GetItem",
                "dynamodb:DeleteItem",
                "dynamodb:PutItem",
                "dynamodb:Scan",
                "dynamodb:Query",
                "dynamodb:UpdateItem",
                "dynamodb:BatchWriteItem",
                "dynamodb:BatchGetItem",
                "dynamodb:DescribeTable",
                "dynamodb:ConditionCheckItem",
            ],
            json!([table(""), table("/index/*")]),
        )],
        "DynamoDBReadPolicy" => vec![statement(
            &[
                "dynamodb:GetItem",
                "dynamodb:Scan",
                "dynamodb:Query",
                "dynamodb:BatchGetItem",
                "dynamodb:DescribeTable",
            ],
            json!([table(""), table("/index/*")]),
        )],
        "DynamoDBWritePolicy" => vec![statement(
            &[
                "dynamodb:PutItem",
                "dynamodb:UpdateItem",
                "dynamodb:BatchWriteItem",
            ],
            json!([table(""), table("/index/*")]),
        )],
        "DynamoDBStreamReadPolicy" => vec![
            statement(
                &[
                    "dynamodb:DescribeStream",
                    "dynamodb:GetRecords",
                    "dynamodb:GetShardIterator",
                ],
                table(&format!("/stream/{}", "*")),
            ),
            statement(&["dynamodb:ListStreams"], json!("*")),
        ],
        "S3ReadPolicy" => vec![statement(
            &[
                "s3:GetObject",
                "s3:ListBucket",
                "s3:GetBucketLocation",
                "s3:GetObjectVersion",
                "s3:GetLifecycleConfiguration",
            ],
            json!([bucket(""), bucket("/*")]),
        )],
        "S3WritePolicy" => vec![statement(
            &[
                "s3:PutObject",
                "s3:PutObjectAcl",
                "s3:PutLifecycleConfiguration",
            ],
            json!([bucket(""), bucket("/*")]),
        )],
        "S3CrudPolicy" => vec![statement(
            &[
                "s3:GetObject",
                "s3:ListBucket",
                "s3:GetBucketLocation",
                "s3:GetObjectVersion",
                "s3:PutObject",
                "s3:PutObjectAcl",
                "s3:GetLifecycleConfiguration",
                "s3:PutLifecycleConfiguration",
                "s3:DeleteObject",
            ],
            json!([bucket(""), bucket("/*")]),
        )],
        "LambdaInvokePolicy" => vec![statement(
            &["lambda:InvokeFunction"],
            arn(
                "lambda:${AWS::Region}:${AWS::AccountId}:function:${functionName}*",
                "functionName",
                arg(args, "FunctionName"),
            ),
        )],
        "StepFunctionsExecutionPolicy" => vec![statement(
            &["states:StartExecution"],
            arn(
                "states:${AWS::Region}:${AWS::AccountId}:stateMachine:${stateMachineName}",
                "stateMachineName",
                arg(args, "StateMachineName"),
            ),
        )],
        "SSMParameterReadPolicy" => vec![
            statement(&["ssm:DescribeParameters"], json!("*")),
            statement(
                &[
                    "ssm:GetParameters",
                    "ssm:GetParameter",
                    "ssm:GetParametersByPath",
                ],
                arn(
                    "ssm:${AWS::Region}:${AWS::AccountId}:parameter/${parameterName}",
                    "parameterName",
                    arg(args, "ParameterName"),
                ),
            ),
        ],
        "SSMParameterWithSlashPrefixReadPolicy" => vec![
            statement(&["ssm:DescribeParameters"], json!("*")),
            statement(
                &[
                    "ssm:GetParameters",
                    "ssm:GetParameter",
                    "ssm:GetParametersByPath",
                ],
                arn(
                    "ssm:${AWS::Region}:${AWS::AccountId}:parameter${parameterName}",
                    "parameterName",
                    arg(args, "ParameterName"),
                ),
            ),
        ],
        "AWSSecretsManagerGetSecretValuePolicy" => vec![statement(
            &["secretsmanager:GetSecretValue"],
            arg(args, "SecretArn"),
        )],
        "KMSDecryptPolicy" => vec![statement(
            &["kms:Decrypt"],
            arn(
                "kms:${AWS::Region}:${AWS::AccountId}:key/${keyId}",
                "keyId",
                arg(args, "KeyId"),
            ),
        )],
        "KMSEncryptPolicy" => vec![statement(
            &["kms:Encrypt"],
            arn(
                "kms:${AWS::Region}:${AWS::AccountId}:key/${keyId}",
                "keyId",
                arg(args, "KeyId"),
            ),
        )],
        "KinesisStreamReadPolicy" => vec![
            statement(
                &["kinesis:ListStreams", "kinesis:DescribeLimits"],
                json!({"Fn::Sub": "arn:${AWS::Partition}:kinesis:${AWS::Region}:${AWS::AccountId}:stream/*"}),
            ),
            statement(
                &[
                    "kinesis:DescribeStream",
                    "kinesis:DescribeStreamSummary",
                    "kinesis:GetRecords",
                    "kinesis:GetShardIterator",
                ],
                arn(
                    "kinesis:${AWS::Region}:${AWS::AccountId}:stream/${streamName}",
                    "streamName",
                    arg(args, "StreamName"),
                ),
            ),
        ],
        "KinesisCrudPolicy" => vec![statement(
            &[
                "kinesis:AddTagsToStream",
                "kinesis:CreateStream",
                "kinesis:DecreaseStreamRetentionPeriod",
                "kinesis:DeleteStream",
                "kinesis:DescribeStream",
                "kinesis:DescribeStreamSummary",
                "kinesis:GetShardIterator",
                "kinesis:IncreaseStreamRetentionPeriod",
                "kinesis:ListTagsForStream",
                "kinesis:MergeShards",
                "kinesis:PutRecord",
                "kinesis:PutRecords",
                "kinesis:SplitShard",
                "kinesis:RemoveTagsFromStream",
            ],
            arn(
                "kinesis:${AWS::Region}:${AWS::AccountId}:stream/${streamName}",
                "streamName",
                arg(args, "StreamName"),
            ),
        )],
        "EventBridgePutEventsPolicy" => vec![statement(
            &["events:PutEvents"],
            arn(
                "events:${AWS::Region}:${AWS::AccountId}:event-bus/${eventBusName}",
                "eventBusName",
                arg(args, "EventBusName"),
            ),
        )],
        "CloudWatchPutMetricPolicy" => vec![statement(&["cloudwatch:PutMetricData"], json!("*"))],
        "AWSXrayWriteOnlyAccess" => vec![statement(
            &[
                "xray:PutTraceSegments",
                "xray:PutTelemetryRecords",
                "xray:GetSamplingRules",
                "xray:GetSamplingTargets",
                "xray:GetSamplingStatisticSummaries",
            ],
            json!("*"),
        )],
        other => {
            return Err(validation(format!(
                "Transform AWS::Serverless-2016-10-31 failed with: policy template {other} is not written by doze-aws; give the policy as a document"
            )));
        }
    };
    Ok(json!({"Version": "2012-10-17", "Statement": statements}))
}
