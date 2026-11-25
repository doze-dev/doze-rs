//! The errors a Fail rule answers with, per service: what each kind of failure is called by the service the client is talking to.
//!
//! Where it comes from, and what is not here:
//!
//! - A **modelled** answer names a shape of the service's own model (`models/*.json`, the vendored AWS models). The gateway renders
//!   it as the model says (wire code, status, fault, the Query code of a Query service), and a test checks that every shape named
//!   here is in the model with the status the table gives.
//! - A **plain** answer is a code the model does not carry, stated with its status. They are the "Common Errors" every service's API
//!   Reference lists (checked on the IAM, STS, Step Functions, CloudFormation, CloudWatch, SNS and SQS pages, 2026-11-25):
//!   `ThrottlingException` 400, `ServiceUnavailable` 503, `InternalFailure` 500, `AccessDeniedException` 403 (SQS's page says 400).
//!   S3 words its own: `SlowDown` (503, "503 (Slow Down)" in the S3 user guide, *Optimizing Amazon S3 performance*),
//!   `ServiceUnavailable`, `InternalError` (the AWS SDKs' transient list, *SDKs and Tools reference guide, Retry behavior*).
//! - **Every throttle here is one SDKs retry as a throttle**: its code is in botocore's `ThrottledRetryableChecker` list (the same
//!   list the SDK reference guide gives for every SDK) or its status is 429 (botocore `ThrottlingStatusCodeChecker`, Java, JS v3).
//!   Every internal error is a 500 and every unavailable a 503, which every SDK retries as transient. An access denied is none of
//!   these: no SDK retries it. A test checks each of those facts of each row.
//!
//! A code not here is a code this table could not verify: use a Custom failure for it.

use std::borrow::Cow;

use aws_gateway::{Ask, Failure, Tag};

use super::rule::Fail;

/// One answer: the model shape it is (when it is one), its status, and the code a client reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Coded {
    /// The shape in the service's model, when the model carries it.
    pub shape: Option<&'static str>,
    /// The HTTP status.
    pub status: u16,
    /// The code (for a Query service, its Query code).
    pub code: &'static str,
}

const fn shape(name: &'static str, status: u16, code: &'static str) -> Coded {
    Coded {
        shape: Some(name),
        status,
        code,
    }
}

const fn plain(status: u16, code: &'static str) -> Coded {
    Coded {
        shape: None,
        status,
        code,
    }
}

/// What one service answers each kind of failure with.
#[derive(Debug, Clone, Copy)]
pub struct Row {
    /// The service's name in a credential scope.
    pub signing: &'static str,
    /// Its SDK id.
    pub sdk_id: &'static str,
    pub throttle: Coded,
    pub unavailable: Coded,
    pub internal: Coded,
    pub denied: Coded,
}

const THROTTLING: Coded = plain(400, "ThrottlingException");
const UNAVAILABLE: Coded = plain(503, "ServiceUnavailable");
const INTERNAL: Coded = plain(500, "InternalFailure");
const DENIED: Coded = plain(403, "AccessDeniedException");

/// What a service the table has no row for answers: the common errors.
pub const COMMON: Row = Row {
    signing: "",
    sdk_id: "",
    throttle: THROTTLING,
    unavailable: UNAVAILABLE,
    internal: INTERNAL,
    denied: DENIED,
};

const fn row(signing: &'static str, sdk_id: &'static str) -> Row {
    Row {
        signing,
        sdk_id,
        ..COMMON
    }
}

const PROVISIONED: Coded = shape(
    "ProvisionedThroughputExceededException",
    400,
    "ProvisionedThroughputExceededException",
);

/// Every service of the catalogue (the signing name and SDK id are the model's).
pub const ROWS: &[Row] = &[
    // The model's own shape `RequestThrottled` (403); botocore lists it, and its legacy SQS policy retries it at 403.
    // SQS's Common Errors page gives AccessDeniedException as 400.
    Row {
        throttle: shape("RequestThrottled", 403, "RequestThrottled"),
        denied: plain(400, "AccessDeniedException"),
        ..row("sqs", "SQS")
    },
    Row {
        throttle: shape("ThrottledException", 429, "Throttled"),
        internal: shape("InternalErrorException", 500, "InternalError"),
        denied: shape("AuthorizationErrorException", 403, "AuthorizationError"),
        ..row("sns", "SNS")
    },
    row("sts", "STS"),
    Row {
        internal: shape("ServiceFailureException", 500, "ServiceFailure"),
        ..row("iam", "IAM")
    },
    Row {
        throttle: PROVISIONED,
        internal: shape("InternalServerError", 500, "InternalServerError"),
        ..row("dynamodb", "DynamoDB")
    },
    Row {
        internal: shape("InternalServerError", 500, "InternalServerError"),
        ..row("dynamodb", "DynamoDB Streams")
    },
    // S3 words its errors its own way, and its model carries none of the three: see the module text.
    Row {
        throttle: plain(503, "SlowDown"),
        internal: plain(500, "InternalError"),
        denied: shape("AccessDenied", 403, "AccessDenied"),
        ..row("s3", "S3")
    },
    Row {
        throttle: PROVISIONED,
        internal: shape("InternalFailureException", 500, "InternalFailureException"),
        denied: shape("AccessDeniedException", 400, "AccessDeniedException"),
        ..row("kinesis", "Kinesis")
    },
    Row {
        internal: shape("KMSInternalException", 500, "KMSInternalException"),
        ..row("kms", "KMS")
    },
    Row {
        throttle: shape("ThrottlingException", 400, "ThrottlingException"),
        internal: shape("InternalServerError", 500, "InternalServerError"),
        denied: shape("AccessDeniedException", 400, "AccessDeniedException"),
        ..row("ssm", "SSM")
    },
    Row {
        internal: shape("InternalServiceError", 500, "InternalServiceError"),
        ..row("secretsmanager", "Secrets Manager")
    },
    Row {
        internal: shape("InternalServiceFault", 500, "InternalServiceFault"),
        ..row("monitoring", "CloudWatch")
    },
    Row {
        throttle: shape("ThrottlingException", 400, "ThrottlingException"),
        internal: shape("InternalServerException", 500, "InternalServerException"),
        denied: shape("AccessDeniedException", 400, "AccessDeniedException"),
        ..row("logs", "CloudWatch Logs")
    },
    Row {
        throttle: shape("ThrottlingException", 400, "ThrottlingException"),
        internal: shape("InternalException", 500, "InternalException"),
        denied: shape("AccessDeniedException", 400, "AccessDeniedException"),
        ..row("events", "EventBridge")
    },
    Row {
        throttle: shape("ThrottlingException", 429, "ThrottlingException"),
        internal: shape("InternalServerException", 500, "InternalServerException"),
        ..row("scheduler", "Scheduler")
    },
    Row {
        throttle: shape("TooManyRequestsException", 429, "TooManyRequestsException"),
        internal: shape("ServiceException", 500, "ServiceException"),
        ..row("lambda", "Lambda")
    },
    row("states", "SFN"),
    row("cloudformation", "CloudFormation"),
    Row {
        throttle: shape("TooManyRequestsException", 429, "TooManyRequestsException"),
        unavailable: shape(
            "ServiceUnavailableException",
            503,
            "ServiceUnavailableException",
        ),
        ..row("apigateway", "API Gateway")
    },
    Row {
        throttle: shape("TooManyRequestsException", 429, "TooManyRequestsException"),
        denied: shape("AccessDeniedException", 403, "AccessDeniedException"),
        ..row("apigateway", "ApiGatewayV2")
    },
    Row {
        throttle: shape("LimitExceededException", 429, "LimitExceededException"),
        ..row("execute-api", "ApiGatewayManagementApi")
    },
];

/// The row of the API called `sdk_id` that signs as `signing`; the common errors when there is none.
#[must_use]
pub fn row_of(signing: &str, sdk_id: &str) -> &'static Row {
    ROWS.iter()
        .find(|r| r.signing == signing && r.sdk_id == sdk_id)
        .or_else(|| ROWS.iter().find(|r| r.signing == signing))
        .unwrap_or(&COMMON)
}

impl Row {
    /// What this service answers a failure of `fail` with; a custom failure is its own.
    #[must_use]
    pub fn answer(&self, fail: &Fail) -> (Option<&'static str>, u16, Cow<'static, str>) {
        let coded = match fail {
            Fail::Throttle => self.throttle,
            Fail::Unavailable => self.unavailable,
            Fail::Internal => self.internal,
            Fail::AccessDenied => self.denied,
            Fail::Custom { status, code } => {
                return (None, *status, Cow::Owned(code.to_string()));
            }
        };
        (coded.shape, coded.status, Cow::Borrowed(coded.code))
    }
}

/// The words of an error, without the mark that says a Lab rule made it.
fn plain_words(fail: &Fail, ask: &Ask<'_>, code: &str) -> String {
    match fail {
        Fail::Throttle => "Rate exceeded.".to_owned(),
        Fail::Unavailable => "The service is temporarily unavailable. Try again later.".to_owned(),
        Fail::Internal => "We encountered an internal error. Please try again.".to_owned(),
        Fail::AccessDenied => format!("The caller is not authorized to perform {}.", ask.operation),
        Fail::Custom { .. } => format!("The request failed with {code}."),
    }
}

/// The failure a rule answers the call `ask` with.
#[must_use]
pub fn failure(fail: &Fail, ask: &Ask<'_>, tag: Tag) -> Failure {
    let (shape, status, code) = row_of(ask.signing_name, ask.sdk_id).answer(fail);
    let message = format!(
        "{} (made by doze-aws Lab rule {})",
        plain_words(fail, ask, &code),
        tag.rule
    );
    Failure {
        shape,
        status,
        code,
        message,
        tag,
    }
}

#[cfg(test)]
mod tests;
