#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use super::*;

const SQS: Model = (&aws_model_sqs::meta::META, &aws_model_sqs::tables::TABLES);
const SNS: Model = (&aws_model_sns::meta::META, &aws_model_sns::tables::TABLES);
const S3: Model = (&aws_model_s3::meta::META, &aws_model_s3::tables::TABLES);
const DDB: Model = (
    &aws_model_dynamodb::meta::META,
    &aws_model_dynamodb::tables::TABLES,
);
const STREAMS: Model = (
    &aws_model_dynamodb_streams::meta::META,
    &aws_model_dynamodb_streams::tables::TABLES,
);

fn codes(list: &[Listed]) -> Vec<&str> {
    list.iter().map(|e| e.code).collect()
}

#[test]
fn an_operation_lists_the_errors_its_model_gives_it_with_the_models_status() {
    let queue = common(&[SQS], &["GetQueueUrl"]);
    let missing = find(&queue, "QueueDoesNotExist").unwrap();
    // The vendored SQS model gives this error 400, and S3's model gives NoSuchKey 404: the model is what is said, not memory.
    assert_eq!((missing.status, missing.server), (400, false));
    let key = common(&[S3], &["GetObject"]);
    assert_eq!(codes(&key), ["InvalidObjectState", "NoSuchKey"]);
    assert_eq!(find(&key, "NoSuchKey").unwrap().status, 404);
    let busy = common(&[DDB], &["GetItem"]);
    let inside = find(&busy, "InternalServerError").unwrap();
    assert_eq!((inside.status, inside.server), (500, true));
}

#[test]
fn several_operations_offer_only_the_errors_every_one_of_them_lists() {
    let both = common(&[SQS], &["SendMessage", "ReceiveMessage"]);
    assert!(find(&both, "QueueDoesNotExist").is_some());
    assert!(find(&both, "RequestThrottled").is_some());
    // One lists it and the other does not.
    assert!(find(&both, "InvalidMessageContents").is_none());
    assert!(find(&both, "OverLimit").is_none());
    let one = common(&[SQS], &["SendMessage"]);
    assert!(find(&one, "InvalidMessageContents").is_some());
    assert!(both.len() < one.len());
    // The order of the operations does not change what they share.
    assert_eq!(both, common(&[SQS], &["ReceiveMessage", "SendMessage"]));
}

#[test]
fn a_query_service_names_a_code_by_its_query_code_and_it_can_still_be_found_by_its_shape() {
    let publish = common(&[SNS], &["Publish"]);
    let invalid = find(&publish, "ParameterValueInvalid").unwrap();
    assert_eq!(invalid.shape, "InvalidParameterValueException");
    assert_eq!(invalid.status, 400);
    assert_eq!(
        find(&publish, "InvalidParameterValueException"),
        Some(invalid)
    );
    // A service that speaks JSON names it by its shape.
    assert!(find(&common(&[DDB], &["GetItem"]), "ResourceNotFoundException").is_some());
    assert!(find(&publish, "NotAnError").is_none());
    assert!(find(&publish, "").is_none());
}

#[test]
fn an_operation_no_model_has_and_no_operation_at_all_offer_nothing() {
    assert!(common(&[SQS], &["SendMesage"]).is_empty());
    assert!(common(&[SQS], &[]).is_empty());
    assert!(common(&[], &["SendMessage"]).is_empty());
}

#[test]
fn two_apis_that_sign_alike_are_each_asked_only_about_the_operations_they_have() {
    // GetItem is DynamoDB's and GetRecords is the Streams'; asked for both in both models, each is asked in the model that has it, and
    // what they share is what is offered.
    let alone = common(&[DDB, STREAMS], &["GetItem"]);
    assert_eq!(alone, common(&[DDB], &["GetItem"]));
    let both = common(&[DDB, STREAMS], &["GetItem", "GetRecords"]);
    let item = common(&[DDB], &["GetItem"]);
    let records = common(&[STREAMS], &["GetRecords"]);
    let shared: Vec<Listed> = item
        .iter()
        .filter(|e| records.contains(e))
        .copied()
        .collect();
    assert_eq!(both, shared);
    assert!(find(&both, "InternalServerError").is_some());
}

#[test]
fn every_error_a_model_lists_for_an_operation_is_one_a_rule_can_answer() {
    let models: Vec<Model> = vec![
        SQS,
        SNS,
        S3,
        DDB,
        STREAMS,
        (&aws_model_sts::meta::META, &aws_model_sts::tables::TABLES),
        (&aws_model_iam::meta::META, &aws_model_iam::tables::TABLES),
        (&aws_model_kms::meta::META, &aws_model_kms::tables::TABLES),
        (
            &aws_model_lambda::meta::META,
            &aws_model_lambda::tables::TABLES,
        ),
        (&aws_model_sfn::meta::META, &aws_model_sfn::tables::TABLES),
        (
            &aws_model_kinesis::meta::META,
            &aws_model_kinesis::tables::TABLES,
        ),
        (&aws_model_ssm::meta::META, &aws_model_ssm::tables::TABLES),
    ];
    let mut checked = 0;
    for model in models {
        let tables = model.1;
        for i in 0..tables.n_ops() {
            let name = tables.op(i).unwrap().name;
            for e in common(&[model], &[name]) {
                assert!((400..=599).contains(&e.status), "{name} {}", e.code);
                assert!(
                    !e.code.is_empty() && e.code.len() <= crate::lab::LONGEST_CODE,
                    "{name} {}",
                    e.code
                );
                assert!(
                    e.code
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '.'),
                    "{name} {}",
                    e.code
                );
                assert_eq!(find(&common(&[model], &[name]), e.code), Some(&e));
                assert_eq!(
                    e.server,
                    e.status >= 500,
                    "{name} {}: the fault of a status",
                    e.code
                );
                checked += 1;
            }
        }
    }
    assert!(checked > 1000, "only {checked} errors were checked");
}
