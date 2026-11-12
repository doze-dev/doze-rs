//! SSM beside the services it calls: KMS seals a `SecureString`, Secrets Manager answers a reference.

use aws_wire::service::Caller;
use serde_json::{Value, json};

use super::World;

fn caller() -> Caller {
    Caller {
        access_key_id: "test".into(),
        account_id: "000000000000".into(),
        region: "us-east-1".into(),
        endpoint: "http://localhost:4566".into(),
        service: None,
        session_token: None,
    }
}

impl World {
    /// A call to another service as a client would make it (not as SSM).
    fn peer(&mut self, target: &str, op: &str, body: &Value) -> Result<Value, aws_wire::ApiError> {
        let peers = self.bus.peers.clone();
        self.bus
            .rt
            .block_on(async move { peers.call("test", &caller(), target, op, body).await })
    }
}

#[test]
fn a_secure_string_is_a_kms_ciphertext_under_the_aws_managed_key() {
    let mut w = World::new();
    // Until SSM first needs it, the AWS-managed key does not exist, and a client may not ask for it.
    assert!(
        w.peer("kms", "DescribeKey", &json!({"KeyId": "alias/aws/ssm"}))
            .is_err()
    );
    w.ok(
        "PutParameter",
        &json!({"Name": "/pw", "Value": "hunter2", "Type": "SecureString"}),
    );
    let key = w
        .peer("kms", "DescribeKey", &json!({"KeyId": "alias/aws/ssm"}))
        .unwrap();
    assert_eq!(key["KeyMetadata"]["KeyManager"], "AWS");
    let sealed = w.ok("GetParameter", &json!({"Name": "/pw"}))["Parameter"]["Value"]
        .as_str()
        .unwrap()
        .to_owned();
    // The value is an envelope: a KMS-sealed data key, made for this parameter, beside the sealed value.
    let stored = aws_core::base64::decode(&sealed).unwrap();
    assert_eq!(stored[0], 1);
    let len = u32::from_be_bytes(stored[1..5].try_into().unwrap()) as usize;
    let blob = aws_core::base64::encode(&stored[5..5 + len]);
    let context = json!({"PARAMETER_ARN": "arn:aws:ssm:us-east-1:000000000000:parameter/pw"});
    let key = w
        .peer(
            "kms",
            "Decrypt",
            &json!({"CiphertextBlob": blob, "EncryptionContext": context}),
        )
        .unwrap();
    assert_eq!(
        aws_core::base64::decode(key["Plaintext"].as_str().unwrap())
            .unwrap()
            .len(),
        32,
        "the data key"
    );
    assert_eq!(
        w.peer("kms", "Decrypt", &json!({"CiphertextBlob": blob}))
            .unwrap_err()
            .code,
        "InvalidCiphertextException",
        "it is bound to the parameter"
    );
}

#[test]
fn a_secure_string_under_a_key_of_the_callers_follows_the_keys_state() {
    let mut w = World::new();
    let id = w.peer("kms", "CreateKey", &json!({})).unwrap()["KeyMetadata"]["KeyId"]
        .as_str()
        .unwrap()
        .to_owned();
    w.ok(
        "PutParameter",
        &json!({"Name": "/own", "Value": "v", "Type": "SecureString", "KeyId": id}),
    );
    assert_eq!(
        w.ok("DescribeParameters", &json!({}))["Parameters"][0]["KeyId"],
        id.as_str()
    );
    w.peer("kms", "DisableKey", &json!({"KeyId": id})).unwrap();
    let (code, message) = w.err(
        "GetParameter",
        &json!({"Name": "/own", "WithDecryption": true}),
    );
    assert_eq!(code, "InvalidKeyId");
    assert!(message.contains("is disabled"), "{message}");
    // The sealed form needs no key to show.
    assert!(w.run("GetParameter", &json!({"Name": "/own"})).is_ok());
    let (code, _) = w.err(
        "PutParameter",
        &json!({"Name": "/own", "Value": "w", "Overwrite": true}),
    );
    assert_eq!(
        code, "InvalidKeyId",
        "an overwrite is sealed again, with the same key"
    );
    w.peer("kms", "EnableKey", &json!({"KeyId": id})).unwrap();
    assert_eq!(
        w.ok(
            "GetParameter",
            &json!({"Name": "/own", "WithDecryption": true})
        )["Parameter"]["Value"],
        "v"
    );
}

#[test]
fn a_secrets_manager_secret_is_read_through_its_reference_name() {
    let mut w = World::new();
    w.peer(
        "secretsmanager",
        "CreateSecret",
        &json!({"Name": "app/db", "SecretString": "s3cret"}),
    )
    .unwrap();
    let name = "/aws/reference/secretsmanager/app/db";
    let got = w.ok(
        "GetParameter",
        &json!({"Name": name, "WithDecryption": true}),
    );
    let p = &got["Parameter"];
    assert_eq!(
        (p["Value"].as_str(), p["Type"].as_str(), p["Name"].as_str()),
        (Some("s3cret"), Some("SecureString"), Some(name))
    );
    assert_eq!(
        w.err("GetParameter", &json!({"Name": name})).0,
        "ValidationException",
        "a secret is never shown sealed"
    );
    let many = w.ok(
        "GetParameters",
        &json!({"Names": [name, "/aws/reference/secretsmanager/absent"], "WithDecryption": true}),
    );
    assert_eq!(many["Parameters"][0]["Value"], "s3cret");
    assert_eq!(
        many["InvalidParameters"],
        json!(["/aws/reference/secretsmanager/absent"])
    );
    assert_eq!(
        w.err(
            "GetParameter",
            &json!({"Name": "/aws/reference/secretsmanager/absent", "WithDecryption": true})
        ),
        ("ParameterNotFound".into(), String::new())
    );
}
