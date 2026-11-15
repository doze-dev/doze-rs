//! Key agreement, last use, and multi-Region keys. The multi-Region tests run the whole service on disk with an actor per region,
//! as the server does: a replica is made by its own region's actor, and the set's shape reaches every region of it.

use std::sync::Arc;

use aws_testkit::Harness;
use aws_wire::service::Service;
use doze_runtime::{Ctx, SystemClock, Tasks};
use serde_json::{Value, json};

use super::{World, temp};
use crate::Kms;

/// KMS on disk, called in any region.
struct Regions {
    kms: Kms,
    h: Harness<Kms>,
    rt: tokio::runtime::Runtime,
    _dir: aws_testkit::TempDir,
}

impl Regions {
    fn new() -> Self {
        let dir = temp();
        let ctx = |path: std::path::PathBuf| {
            Arc::new(Ctx {
                dir: path,
                idle: std::time::Duration::from_secs(300),
                tasks: Tasks::new(),
                clock: Arc::new(SystemClock),
                ext: None,
            })
        };
        Self {
            kms: Kms::new(ctx(dir.path().join("kms"))),
            h: Harness::new(Kms::new(ctx(dir.path().join("unused")))),
            rt: tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap(),
            _dir: dir,
        }
    }

    fn run(&self, region: &str, op: &str, request: &Value) -> Result<Value, aws_wire::ApiError> {
        let ix = self.h.op(op);
        let mut call = self.h.call(ix, request)?;
        call.caller.region = region.to_owned();
        let out = self.rt.block_on(self.kms.call(call))?;
        Ok(self.h.json(ix, &out))
    }

    fn ok(&self, region: &str, op: &str, request: &Value) -> Value {
        self.run(region, op, request)
            .unwrap_or_else(|e| panic!("{op} in {region}: {} {}", e.code, e.message))
    }

    fn code(&self, region: &str, op: &str, request: &Value) -> String {
        let e = self.run(region, op, request).unwrap_err();
        e.shape
            .map_or_else(|| e.code.to_string(), |s| s.to_string())
    }

    fn config(&self, region: &str, id: &str) -> Value {
        self.ok(region, "DescribeKey", &json!({"KeyId": id}))["KeyMetadata"]["MultiRegionConfiguration"].clone()
    }
}

const EAST: &str = "us-east-1";
const WEST: &str = "us-west-2";
const EU: &str = "eu-west-1";

#[test]
fn a_replica_opens_what_its_primary_sealed_and_follows_its_rotation() {
    let r = Regions::new();
    let made = r.ok(EAST, "CreateKey", &json!({"MultiRegion": true}));
    let id = made["KeyMetadata"]["KeyId"].as_str().unwrap().to_owned();
    assert!(id.starts_with("mrk-"));
    let replica = r.ok(
        EAST,
        "ReplicateKey",
        &json!({"KeyId": id, "ReplicaRegion": WEST, "Description": "west",
        "Tags": [{"TagKey": "k", "TagValue": "v"}]}),
    );
    let meta = &replica["ReplicaKeyMetadata"];
    assert_eq!(
        meta["Arn"],
        format!("arn:aws:kms:{WEST}:000000000000:key/{id}")
    );
    assert_eq!(
        meta["MultiRegionConfiguration"]["MultiRegionKeyType"],
        "REPLICA"
    );
    assert_eq!(meta["Description"], "west");
    assert_eq!(
        replica["ReplicaTags"],
        json!([{"TagKey": "k", "TagValue": "v"}])
    );
    let east = r.config(EAST, &id);
    assert_eq!(east["MultiRegionKeyType"], "PRIMARY");
    assert_eq!(east["ReplicaKeys"][0]["Region"], WEST);
    assert_eq!(r.config(WEST, &id)["PrimaryKey"]["Region"], EAST);
    let sealed =
        r.ok(EAST, "Encrypt", &json!({"KeyId": id, "Plaintext": "aGk="}))["CiphertextBlob"].clone();
    assert_eq!(
        r.ok(WEST, "Decrypt", &json!({"CiphertextBlob": sealed}))["Plaintext"],
        "aGk="
    );
    r.ok(EAST, "RotateKeyOnDemand", &json!({"KeyId": id}));
    let after =
        r.ok(EAST, "Encrypt", &json!({"KeyId": id, "Plaintext": "aGk="}))["CiphertextBlob"].clone();
    assert_eq!(
        r.ok(WEST, "Decrypt", &json!({"CiphertextBlob": after}))["Plaintext"],
        "aGk="
    );
    assert_eq!(
        r.code(WEST, "RotateKeyOnDemand", &json!({"KeyId": id})),
        "UnsupportedOperationException"
    );
    assert_eq!(
        r.code(
            EAST,
            "ReplicateKey",
            &json!({"KeyId": id, "ReplicaRegion": WEST})
        ),
        "AlreadyExistsException"
    );
    assert_eq!(
        r.code(
            WEST,
            "ReplicateKey",
            &json!({"KeyId": id, "ReplicaRegion": EU})
        ),
        "UnsupportedOperationException"
    );
    let single = r.ok(EAST, "CreateKey", &json!({}))["KeyMetadata"]["KeyId"].clone();
    assert_eq!(
        r.code(
            EAST,
            "ReplicateKey",
            &json!({"KeyId": single, "ReplicaRegion": WEST})
        ),
        "UnsupportedOperationException"
    );
}

#[test]
fn the_primary_moves_and_every_region_sees_the_new_shape() {
    let r = Regions::new();
    let id = r.ok(EAST, "CreateKey", &json!({"MultiRegion": true}))["KeyMetadata"]["KeyId"]
        .as_str()
        .unwrap()
        .to_owned();
    r.ok(
        EAST,
        "ReplicateKey",
        &json!({"KeyId": id, "ReplicaRegion": WEST}),
    );
    r.ok(
        EAST,
        "ReplicateKey",
        &json!({"KeyId": id, "ReplicaRegion": EU}),
    );
    assert_eq!(
        r.config(WEST, &id)["ReplicaKeys"].as_array().unwrap().len(),
        2
    );
    assert_eq!(
        r.code(
            EAST,
            "UpdatePrimaryRegion",
            &json!({"KeyId": id, "PrimaryRegion": "ap-south-1"})
        ),
        "UnsupportedOperationException"
    );
    r.ok(
        EAST,
        "UpdatePrimaryRegion",
        &json!({"KeyId": id, "PrimaryRegion": WEST}),
    );
    for region in [EAST, WEST, EU] {
        assert_eq!(
            r.config(region, &id)["PrimaryKey"]["Region"],
            WEST,
            "{region}"
        );
    }
    assert_eq!(r.config(EAST, &id)["MultiRegionKeyType"], "REPLICA");
    assert_eq!(
        r.code(WEST, "ScheduleKeyDeletion", &json!({"KeyId": id})),
        "UnsupportedOperationException"
    );
    r.ok(
        EU,
        "ScheduleKeyDeletion",
        &json!({"KeyId": id, "PendingWindowInDays": 7}),
    );
    let replicas = r.config(WEST, &id)["ReplicaKeys"].clone();
    assert_eq!(
        replicas,
        json!([{"Arn": format!("arn:aws:kms:{EAST}:000000000000:key/{id}"), "Region": EAST}])
    );
    r.ok(EU, "CancelKeyDeletion", &json!({"KeyId": id}));
    assert_eq!(
        r.config(WEST, &id)["ReplicaKeys"].as_array().unwrap().len(),
        2
    );
}

#[test]
fn ecdh_derives_the_secret_the_peer_derives() {
    let mut w = World::new();
    let ours = w.key(&json!({"KeySpec": "ECC_NIST_P384", "KeyUsage": "KEY_AGREEMENT"}));
    let theirs = w.key(&json!({"KeySpec": "ECC_NIST_P384", "KeyUsage": "KEY_AGREEMENT"}));
    let meta = w.ok("DescribeKey", &json!({"KeyId": ours}))["KeyMetadata"].clone();
    assert_eq!(meta["KeyAgreementAlgorithms"], json!(["ECDH"]));
    assert!(meta.get("SigningAlgorithms").is_none());
    let pub_ours = w.ok("GetPublicKey", &json!({"KeyId": ours}));
    assert_eq!(pub_ours["KeyAgreementAlgorithms"], json!(["ECDH"]));
    let pub_theirs = w.ok("GetPublicKey", &json!({"KeyId": theirs}))["PublicKey"].clone();
    let a = w.ok(
        "DeriveSharedSecret",
        &json!({"KeyId": ours, "KeyAgreementAlgorithm": "ECDH", "PublicKey": pub_theirs}),
    );
    let b = w.ok("DeriveSharedSecret", &json!({"KeyId": theirs, "KeyAgreementAlgorithm": "ECDH", "PublicKey": pub_ours["PublicKey"]}));
    assert_eq!(a["SharedSecret"], b["SharedSecret"]);
    assert_eq!(
        (a["KeyAgreementAlgorithm"].as_str(), a["KeyOrigin"].as_str()),
        (Some("ECDH"), Some("AWS_KMS"))
    );
    let other = w.key(&json!({"KeySpec": "ECC_NIST_P256", "KeyUsage": "KEY_AGREEMENT"}));
    let p256 = w.ok("GetPublicKey", &json!({"KeyId": other}))["PublicKey"].clone();
    let derive = |key: &str, public: &Value| json!({"KeyId": key, "KeyAgreementAlgorithm": "ECDH", "PublicKey": public});
    assert_eq!(
        w.err("DeriveSharedSecret", &derive(&ours, &p256)).0,
        "ValidationException"
    );
    let signer = w.key(&json!({"KeySpec": "ECC_NIST_P384", "KeyUsage": "SIGN_VERIFY"}));
    assert_eq!(
        w.err("DeriveSharedSecret", &derive(&signer, &pub_theirs)).0,
        "InvalidKeyUsageException"
    );
    let mut dry = derive(&ours, &pub_theirs);
    dry["DryRun"] = json!(true);
    assert_eq!(
        w.err("DeriveSharedSecret", &dry).0,
        "DryRunOperationException"
    );
    assert_eq!(
        w.err(
            "CreateKey",
            &json!({"KeySpec": "ECC_SECG_P256K1", "KeyUsage": "KEY_AGREEMENT"})
        )
        .0,
        "ValidationException"
    );
}

#[test]
fn the_last_use_of_a_key_is_answered() {
    let r = Regions::new();
    let id = r.ok(EAST, "CreateKey", &json!({}))["KeyMetadata"]["KeyId"]
        .as_str()
        .unwrap()
        .to_owned();
    let before = r.ok(EAST, "GetKeyLastUsage", &json!({"KeyId": id}));
    assert_eq!(before["KeyLastUsage"], json!({}));
    assert!(
        before["TrackingStartDate"].as_f64().unwrap()
            >= before["KeyCreationDate"].as_f64().unwrap()
    );
    let sealed =
        r.ok(EAST, "Encrypt", &json!({"KeyId": id, "Plaintext": "aGk="}))["CiphertextBlob"].clone();
    assert_eq!(
        r.ok(EAST, "GetKeyLastUsage", &json!({"KeyId": id}))["KeyLastUsage"]["Operation"],
        "Encrypt"
    );
    r.ok(EAST, "Decrypt", &json!({"CiphertextBlob": sealed}));
    let after = r.ok(EAST, "GetKeyLastUsage", &json!({"KeyId": id}));
    assert_eq!(after["KeyLastUsage"]["Operation"], "Decrypt");
    assert!(after["KeyLastUsage"]["KmsRequestId"].is_string());
    r.ok(EAST, "DescribeKey", &json!({"KeyId": id}));
    assert_eq!(
        r.ok(EAST, "GetKeyLastUsage", &json!({"KeyId": id}))["KeyLastUsage"]["Operation"],
        "Decrypt",
        "a describe is not a use"
    );
    assert_eq!(
        r.code(EAST, "GetKeyLastUsage", &json!({"KeyId": "alias/aws/ssm"})),
        "ValidationException"
    );
}

#[test]
fn multi_region_sets_survive_a_restart() {
    let dir = temp();
    let clock = std::sync::Arc::new(doze_runtime::ManualClock::at(super::START));
    let mut core = crate::core::Core::open_in(dir.path(), clock.clone()).unwrap();
    let set = crate::multi::Set {
        primary: EAST.into(),
        replicas: vec![WEST.into()],
    };
    let mut w = World::with(core, clock.clone());
    let id = w.key(&json!({"MultiRegion": true}));
    w.core.take_set(&id, set.clone());
    if let Some(saved) = w.core.flush().unwrap() {
        saved.wait().unwrap();
    }
    w.core.db.take().unwrap().close();
    core = crate::core::Core::open_in(dir.path(), clock).unwrap();
    assert_eq!(core.sets.get(&id), Some(&set));
    let (_, bytes) = core.count();
    assert!(bytes > 0);
    core.wipe().unwrap();
    assert!(core.sets.is_empty());
}
