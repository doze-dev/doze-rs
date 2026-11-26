#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use aws_gateway::Ask;
use aws_schema::meta::{Protocol, ServiceMeta};
use aws_schema::tables::Tables;

use super::*;
use crate::lab::testing::ask;

/// The vendored model of every service of the build.
fn models() -> Vec<(&'static ServiceMeta, &'static Tables)> {
    vec![
        (&aws_model_sqs::meta::META, &aws_model_sqs::tables::TABLES),
        (&aws_model_sns::meta::META, &aws_model_sns::tables::TABLES),
        (&aws_model_sts::meta::META, &aws_model_sts::tables::TABLES),
        (&aws_model_iam::meta::META, &aws_model_iam::tables::TABLES),
        (
            &aws_model_dynamodb::meta::META,
            &aws_model_dynamodb::tables::TABLES,
        ),
        (
            &aws_model_dynamodb_streams::meta::META,
            &aws_model_dynamodb_streams::tables::TABLES,
        ),
        (&aws_model_s3::meta::META, &aws_model_s3::tables::TABLES),
        (
            &aws_model_kinesis::meta::META,
            &aws_model_kinesis::tables::TABLES,
        ),
        (&aws_model_kms::meta::META, &aws_model_kms::tables::TABLES),
        (&aws_model_ssm::meta::META, &aws_model_ssm::tables::TABLES),
        (
            &aws_model_secrets_manager::meta::META,
            &aws_model_secrets_manager::tables::TABLES,
        ),
        (
            &aws_model_cloudwatch::meta::META,
            &aws_model_cloudwatch::tables::TABLES,
        ),
        (
            &aws_model_cloudwatch_logs::meta::META,
            &aws_model_cloudwatch_logs::tables::TABLES,
        ),
        (
            &aws_model_eventbridge::meta::META,
            &aws_model_eventbridge::tables::TABLES,
        ),
        (
            &aws_model_scheduler::meta::META,
            &aws_model_scheduler::tables::TABLES,
        ),
        (
            &aws_model_lambda::meta::META,
            &aws_model_lambda::tables::TABLES,
        ),
        (&aws_model_sfn::meta::META, &aws_model_sfn::tables::TABLES),
        (
            &aws_model_cloudformation::meta::META,
            &aws_model_cloudformation::tables::TABLES,
        ),
        (
            &aws_model_api_gateway::meta::META,
            &aws_model_api_gateway::tables::TABLES,
        ),
        (
            &aws_model_apigatewayv2::meta::META,
            &aws_model_apigatewayv2::tables::TABLES,
        ),
        (
            &aws_model_apigatewaymanagementapi::meta::META,
            &aws_model_apigatewaymanagementapi::tables::TABLES,
        ),
    ]
}

/// botocore's `ThrottledRetryableChecker._THROTTLED_ERROR_CODES` (`botocore/retries/standard.py`), the list the AWS SDKs and Tools
/// reference guide gives for every SDK, read from the botocore source on 2026-11-25.
const THROTTLED: &[&str] = &[
    "Throttling",
    "ThrottlingException",
    "ThrottledException",
    "RequestThrottledException",
    "TooManyRequestsException",
    "ProvisionedThroughputExceededException",
    "TransactionInProgressException",
    "RequestLimitExceeded",
    "BandwidthLimitExceeded",
    "LimitExceededException",
    "RequestThrottled",
    "SlowDown",
    "PriorRequestNotComplete",
    "EC2ThrottledException",
];

/// botocore's `TransientRetryableChecker._TRANSIENT_ERROR_CODES`.
const TRANSIENT: &[&str] = &[
    "RequestTimeout",
    "RequestTimeoutException",
    "PriorRequestNotComplete",
];

fn each_coded(mut f: impl FnMut(&Row, &str, &Coded)) {
    for row in ROWS {
        for (kind, c) in [
            ("throttle", &row.throttle),
            ("unavailable", &row.unavailable),
            ("internal", &row.internal),
            ("denied", &row.denied),
        ] {
            f(row, kind, c);
        }
    }
}

#[test]
fn every_service_of_the_build_has_its_row_and_no_row_is_for_a_service_that_is_not_there() {
    let models = models();
    assert_eq!(ROWS.len(), models.len());
    for (meta, _) in &models {
        let rows: Vec<&Row> = ROWS
            .iter()
            .filter(|r| r.signing == meta.signing_name && r.sdk_id == meta.sdk_id)
            .collect();
        assert_eq!(rows.len(), 1, "{} ({})", meta.sdk_id, meta.signing_name);
    }
}

#[test]
fn every_shape_a_row_names_is_in_its_model_with_the_status_and_code_the_model_gives() {
    let mut checked = 0;
    for (meta, tables) in models() {
        let row = row_of(meta.signing_name, meta.sdk_id);
        assert_eq!(row.sdk_id, meta.sdk_id);
        for (kind, c) in [
            ("throttle", &row.throttle),
            ("unavailable", &row.unavailable),
            ("internal", &row.internal),
            ("denied", &row.denied),
        ] {
            let Some(name) = c.shape else { continue };
            let found = (0..tables.n_errs())
                .filter_map(|i| tables.err(i))
                .find(|e| tables.shape(e.shape).is_some_and(|s| s.name == name));
            let Some(e) = found else {
                panic!("{} {kind}: no error shape {name} in the model", meta.sdk_id);
            };
            assert_eq!(
                c.status, e.status,
                "{} {kind} {name}: the model's status",
                meta.sdk_id
            );
            // A service that speaks only Query words the error by its Query code; the others by the shape's name. CloudWatch speaks
            // both: its row has the name the JSON and CBOR protocols use.
            let query_only = meta.speaks(Protocol::AwsQuery)
                && !meta.speaks(Protocol::AwsJson10)
                && !meta.speaks(Protocol::AwsJson11)
                && !meta.speaks(Protocol::Rpcv2Cbor);
            let want = if query_only { e.code } else { name };
            assert_eq!(c.code, want, "{} {kind} {name}: the code", meta.sdk_id);
            checked += 1;
        }
    }
    assert!(
        checked >= 30,
        "only {checked} modelled answers were checked"
    );
}

#[test]
fn a_throttle_is_one_every_sdk_retries_as_a_throttle() {
    each_coded(|row, kind, c| {
        if kind == "throttle" {
            assert!(
                THROTTLED.contains(&c.code)
                    || c.status == 429
                    || c.shape.is_some_and(|s| THROTTLED.contains(&s)),
                "{} {}: {} {}",
                row.sdk_id,
                kind,
                c.status,
                c.code
            );
        }
    });
}

#[test]
fn an_internal_error_is_a_500_and_an_unavailable_a_503_which_every_sdk_retries() {
    each_coded(|row, kind, c| match kind {
        "internal" => assert_eq!(c.status, 500, "{} {}", row.sdk_id, c.code),
        "unavailable" => assert_eq!(c.status, 503, "{} {}", row.sdk_id, c.code),
        _ => {}
    });
}

#[test]
fn an_access_denied_is_a_code_no_sdk_retries() {
    each_coded(|row, kind, c| {
        if kind != "denied" {
            return;
        }
        assert!(
            c.code.starts_with("AccessDenied") || c.code == "AuthorizationError",
            "{}: {}",
            row.sdk_id,
            c.code
        );
        assert!(!THROTTLED.contains(&c.code) && !TRANSIENT.contains(&c.code));
        assert!(
            (400..500).contains(&c.status) && c.status != 429 && c.status != 408,
            "{}: {}",
            row.sdk_id,
            c.status
        );
    });
}

#[test]
fn a_service_the_table_does_not_know_answers_the_common_errors() {
    let r = row_of("nope", "Nope");
    assert_eq!(r.throttle, COMMON.throttle);
    assert_eq!(
        (r.throttle.status, r.throttle.code),
        (400, "ThrottlingException")
    );
    assert_eq!(
        (r.unavailable.status, r.unavailable.code),
        (503, "ServiceUnavailable")
    );
    assert_eq!(
        (r.internal.status, r.internal.code),
        (500, "InternalFailure")
    );
    assert_eq!(
        (r.denied.status, r.denied.code),
        (403, "AccessDeniedException")
    );
}

#[test]
fn two_apis_that_sign_alike_each_have_their_own_row() {
    assert_eq!(
        row_of("dynamodb", "DynamoDB").throttle.code,
        "ProvisionedThroughputExceededException"
    );
    assert_eq!(
        row_of("dynamodb", "DynamoDB Streams").throttle.code,
        "ThrottlingException"
    );
    assert_eq!(
        row_of("apigateway", "API Gateway").unavailable.shape,
        Some("ServiceUnavailableException")
    );
    assert_eq!(row_of("apigateway", "ApiGatewayV2").unavailable.shape, None);
}

#[test]
fn a_failure_carries_the_rule_and_says_it_was_made_by_one() {
    let call: Ask<'_> = ask("sqs", "SQS", "SendMessage");
    let tag = || aws_gateway::Tag {
        rule: 12,
        name: "n".into(),
    };
    for fail in [
        Fail::Throttle,
        Fail::Unavailable,
        Fail::Internal,
        Fail::AccessDenied,
    ] {
        let f = failure(&fail, &call, tag());
        assert!(
            f.message.ends_with("(made by doze-aws Lab rule 12)"),
            "{}",
            f.message
        );
        assert_eq!(f.tag.rule, 12);
    }
    let denied = failure(&Fail::AccessDenied, &call, tag());
    assert!(denied.message.contains("SendMessage"));
    let custom = failure(
        &Fail::Custom {
            status: 509,
            code: "Bandwidth.Exceeded".into(),
            shape: None,
        },
        &call,
        tag(),
    );
    assert_eq!(
        (custom.shape, custom.status, &*custom.code),
        (None, 509, "Bandwidth.Exceeded")
    );
    assert!(custom.message.contains("Bandwidth.Exceeded"));
}

#[test]
fn an_error_chosen_from_the_model_keeps_its_shape_so_the_gateway_words_it_as_the_model_does() {
    let tag = || aws_gateway::Tag {
        rule: 3,
        name: "n".into(),
    };
    let call = ask("sqs", "SQS", "SendMessage");
    let chosen = failure(
        &Fail::Custom {
            status: 400,
            code: "InvalidMessageContents".into(),
            shape: Some("InvalidMessageContents"),
        },
        &call,
        tag(),
    );
    assert_eq!(
        (chosen.shape, chosen.status, &*chosen.code),
        (Some("InvalidMessageContents"), 400, "InvalidMessageContents")
    );
    assert!(chosen.message.contains("InvalidMessageContents"));
}
