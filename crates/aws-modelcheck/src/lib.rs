//! All 18 generated models in one list, for tests that hold them against the decoder and the raw models.

use aws_schema::meta::ServiceMeta;
use aws_schema::tables::Tables;

/// One service's generated tables.
#[derive(Debug, Clone, Copy)]
pub struct Service {
    /// The model file's stem (`models/<name>.json`).
    pub name: &'static str,
    pub tables: &'static Tables,
    pub meta: &'static ServiceMeta,
    /// How many operations the model defines (the length of its `Operation::ALL`).
    pub operations: usize,
    /// The `Operation` enum's variants as text, in `ALL` order.
    pub operation_names: fn() -> Vec<String>,
}

macro_rules! list {
    ($($name:literal => $krate:ident),* $(,)?) => {
        vec![$(Service {
            name: $name,
            tables: &$krate::tables::TABLES,
            meta: &$krate::meta::META,
            operations: $krate::Operation::ALL.len(),
            operation_names: || $krate::Operation::ALL.iter().map(|o| format!("{o:?}")).collect(),
        }),*]
    };
}

/// The services, in name order.
#[must_use]
pub fn services() -> Vec<Service> {
    list![
        "api-gateway" => aws_model_api_gateway,
        "apigatewayv2" => aws_model_apigatewayv2,
        "cloudformation" => aws_model_cloudformation,
        "cloudwatch" => aws_model_cloudwatch,
        "cloudwatch-logs" => aws_model_cloudwatch_logs,
        "dynamodb" => aws_model_dynamodb,
        "eventbridge" => aws_model_eventbridge,
        "iam" => aws_model_iam,
        "kinesis" => aws_model_kinesis,
        "kms" => aws_model_kms,
        "lambda" => aws_model_lambda,
        "s3" => aws_model_s3,
        "secrets-manager" => aws_model_secrets_manager,
        "sfn" => aws_model_sfn,
        "sns" => aws_model_sns,
        "sqs" => aws_model_sqs,
        "ssm" => aws_model_ssm,
        "sts" => aws_model_sts,
    ]
}
