//! Secrets Manager beside KMS: every value is kept under a KMS key, and follows that key's state.

use aws_wire::service::Caller;
use serde_json::{Value, json};

use super::World;

impl World {
    /// A call to KMS as a client would make it (not as Secrets Manager).
    fn kms(&mut self, op: &str, body: &Value) -> Result<Value, aws_wire::ApiError> {
        let caller = Caller {
            access_key_id: "test".into(),
            account_id: "000000000000".into(),
            region: "us-east-1".into(),
            endpoint: "http://localhost:4566".into(),
            service: None,
            session_token: None,
        };
        let peers = self.bus.peers.clone();
        self.bus
            .rt
            .block_on(async move { peers.call("test", &caller, "kms", op, body).await })
    }
}

#[test]
fn a_secret_is_kept_under_the_aws_managed_key_until_one_is_named() {
    let mut w = World::new();
    assert!(
        w.kms("DescribeKey", &json!({"KeyId": "alias/aws/secretsmanager"}))
            .is_err(),
        "no such key yet, and a client may not make it"
    );
    w.create("first", "v");
    let key = w
        .kms("DescribeKey", &json!({"KeyId": "alias/aws/secretsmanager"}))
        .unwrap();
    assert_eq!(
        (
            key["KeyMetadata"]["KeyManager"].as_str(),
            key["KeyMetadata"]["KeyState"].as_str()
        ),
        (Some("AWS"), Some("Enabled"))
    );
    // What is stored is not the value.
    let sealed = &w
        .core
        .secrets
        .get("first")
        .unwrap()
        .versions
        .values()
        .next()
        .unwrap()
        .sealed;
    assert!(
        sealed.len() > 40 && sealed.first() == Some(&1),
        "an envelope: a version byte, a KMS-sealed data key, the sealed value"
    );
}

#[test]
fn a_secret_under_a_key_of_the_callers_follows_the_keys_state() {
    let mut w = World::new();
    let id = w.kms("CreateKey", &json!({})).unwrap()["KeyMetadata"]["KeyId"]
        .as_str()
        .unwrap()
        .to_owned();
    w.ok(
        "CreateSecret",
        &json!({"Name": "own", "SecretString": "v", "KmsKeyId": id}),
    );
    assert_eq!(w.get("own")["SecretString"], "v");
    w.kms("DisableKey", &json!({"KeyId": id})).unwrap();
    let (code, message) = w.err("GetSecretValue", &json!({"SecretId": "own"}));
    assert_eq!(code, "DecryptionFailure");
    assert!(message.contains("is disabled"), "{message}");
    let (code, _) = w.err(
        "PutSecretValue",
        &json!({"SecretId": "own", "SecretString": "w"}),
    );
    assert_eq!(code, "EncryptionFailure");
    // A refused write changed nothing.
    assert_eq!(
        w.ok("ListSecretVersionIds", &json!({"SecretId": "own"}))["Versions"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    w.kms("EnableKey", &json!({"KeyId": id})).unwrap();
    assert_eq!(w.get("own")["SecretString"], "v");
    // Moving a secret to another key is an update that names it, and the new version is sealed under it.
    let other = w.kms("CreateKey", &json!({})).unwrap()["KeyMetadata"]["KeyId"]
        .as_str()
        .unwrap()
        .to_owned();
    w.ok(
        "UpdateSecret",
        &json!({"SecretId": "own", "KmsKeyId": other, "SecretString": "moved"}),
    );
    w.kms("DisableKey", &json!({"KeyId": id})).unwrap();
    assert_eq!(
        w.get("own")["SecretString"],
        "moved",
        "the old key no longer matters to the current version"
    );
}

#[test]
fn a_value_cannot_be_moved_to_another_secret() {
    let mut w = World::new();
    w.create("a", "alpha");
    w.create("b", "beta");
    // The encryption context binds a value to its secret and version: another secret's stored bytes do not open here.
    let stolen = w
        .core
        .secrets
        .get("a")
        .unwrap()
        .versions
        .values()
        .next()
        .unwrap()
        .sealed
        .clone();
    let vid = w
        .core
        .secrets
        .get("b")
        .unwrap()
        .versions
        .keys()
        .next()
        .unwrap()
        .clone();
    w.core
        .secrets
        .get_mut("b")
        .unwrap()
        .versions
        .get_mut(&vid)
        .unwrap()
        .sealed = stolen;
    assert_eq!(
        w.err("GetSecretValue", &json!({"SecretId": "b"})).0,
        "DecryptionFailure"
    );
}
