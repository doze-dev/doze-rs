#![allow(clippy::unwrap_used)]
use aws_model_sqs::tables::TABLES;
use aws_schema::json::decode;
use aws_schema::patterns::Patterns;

use super::*;

#[test]
fn a_missing_member_is_named() {
    let shape = TABLES
        .op(TABLES.find_op("SendMessage").unwrap())
        .unwrap()
        .input
        .unwrap();
    let v = decode(
        &TABLES,
        &Patterns::new(&TABLES),
        shape,
        r#"{"MessageBody":"x"}"#,
        &mut Arena::new(),
    )
    .unwrap_err();
    let e = default_invalid(&TABLES, &v);
    assert_eq!((e.status, e.code.as_ref()), (400, "ValidationException"));
    assert_eq!(
        e.message,
        "The request must contain the parameter QueueUrl."
    );
}

#[test]
fn every_holder_of_a_models_patterns_shares_one_set_compiled_once() {
    use aws_model_lambda::tables::TABLES as LAMBDA;
    let (gateway, service) = (Patterns::new(&LAMBDA), Patterns::new(&LAMBDA));
    let shape = LAMBDA
        .op(LAMBDA.find_op("GetFunction").unwrap())
        .unwrap()
        .input
        .unwrap();
    decode(
        &LAMBDA,
        &gateway,
        shape,
        r#"{"FunctionName":"orders"}"#,
        &mut Arena::new(),
    )
    .unwrap();
    // What the gateway compiled, the service holds.
    assert!(service.allocated());
}
