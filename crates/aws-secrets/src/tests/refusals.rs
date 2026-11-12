//! What is refused. Messages AWS gave when the scenarios were recorded are asserted whole.

use serde_json::json;

use super::World;

#[test]
fn what_aws_refused_it_refuses_in_its_words() {
    let mut w = World::new();
    let absent = |w: &mut World, op: &str| w.err(op, &json!({"SecretId": "absent"}));
    assert_eq!(
        absent(&mut w, "GetSecretValue"),
        (
            "ResourceNotFoundException".into(),
            "Secrets Manager can't find the specified secret.".into()
        )
    );
    assert_eq!(
        absent(&mut w, "DescribeSecret").0,
        "ResourceNotFoundException"
    );
    let (code, message) = w.err(
        "CreateSecret",
        &json!({"Name": "has space!", "SecretString": "x"}),
    );
    assert_eq!(
        (code.as_str(), message.as_str()),
        (
            "ValidationException",
            "Invalid name. Must be a valid name containing alphanumeric characters, or any of the following: -/_+=.@!"
        )
    );
}

#[test]
fn what_it_refuses_of_a_secret_that_exists() {
    let mut w = World::new();
    w.create("s", "v");
    assert_eq!(
        w.err(
            "CreateSecret",
            &json!({"Name": "s", "SecretString": "other"})
        ),
        (
            "ResourceExistsException".into(),
            "The operation failed because the secret s already exists.".into()
        )
    );
    assert_eq!(
        w.err("PutSecretValue", &json!({"SecretId": "s", "SecretString": "x", "SecretBinary": "AAH+"})),
        ("InvalidParameterException".into(), "You can't specify both a binary secret value and a string secret value in the same secret.".into())
    );
    assert_eq!(
        w.err(
            "GetSecretValue",
            &json!({"SecretId": "s", "VersionStage": "NOSUCHSTAGE"})
        )
        .1,
        "Secrets Manager can't find the specified secret value for staging label: NOSUCHSTAGE"
    );
    let id = "00000000-0000-4000-8000-000000000000";
    assert_eq!(
        w.err("GetSecretValue", &json!({"SecretId": "s", "VersionId": id}))
            .1,
        format!("Secrets Manager can't find the specified secret value for VersionId: {id}")
    );
    assert_eq!(
        w.err(
            "DeleteSecret",
            &json!({"SecretId": "s", "RecoveryWindowInDays": 7, "ForceDeleteWithoutRecovery": true})
        ),
        (
            "InvalidParameterException".into(),
            "You can't use ForceDeleteWithoutRecovery in conjunction with RecoveryWindowInDays."
                .into()
        )
    );
    assert_eq!(
        w.err(
            "DeleteSecret",
            &json!({"SecretId": "s", "RecoveryWindowInDays": 3})
        ),
        (
            "InvalidParameterException".into(),
            "The RecoveryWindowInDays value must be between 7 and 30 days (inclusive).".into()
        )
    );
    w.ok(
        "DeleteSecret",
        &json!({"SecretId": "s", "RecoveryWindowInDays": 7}),
    );
    let marked =
        "You can't perform this operation on the secret because it was marked for deletion.";
    assert_eq!(
        w.err("GetSecretValue", &json!({"SecretId": "s"})),
        ("InvalidRequestException".into(), marked.into())
    );
    assert_eq!(
        w.err(
            "PutSecretValue",
            &json!({"SecretId": "s", "SecretString": "x"})
        ),
        ("InvalidRequestException".into(), marked.into())
    );
}
