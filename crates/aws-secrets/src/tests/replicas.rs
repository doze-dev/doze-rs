//! Replicas: the whole service runs on disk with an actor per region and KMS beside it, as the server runs it. A replica is made
//! in its own region, follows its primary's changes, is read-only there, reports its status on the primary, and can be removed
//! or promoted.

use std::sync::Arc;

use aws_peers::Peers;
use aws_testkit::Harness;
use aws_wire::service::Service;
use doze_runtime::{Ctx, SystemClock, Tasks};
use serde_json::{Value, json};

use super::temp;
use crate::Secrets;

const EAST: &str = "us-east-1";
const WEST: &str = "us-west-2";
const EU: &str = "eu-west-1";

struct Regions {
    secrets: Arc<Secrets>,
    h: Harness<Secrets>,
    rt: tokio::runtime::Runtime,
    _dir: aws_testkit::TempDir,
}

impl Regions {
    fn new() -> Self {
        let dir = temp();
        let peers = Peers::new();
        let ext: Arc<dyn std::any::Any + Send + Sync> = peers.clone();
        let ctx = |name: &str, ext: Option<Arc<dyn std::any::Any + Send + Sync>>| {
            Arc::new(Ctx {
                dir: dir.path().join(name),
                idle: std::time::Duration::from_secs(300),
                tasks: Tasks::new(),
                clock: Arc::new(SystemClock),
                ext,
            })
        };
        peers.register(Arc::new(aws_kms::Kms::new(ctx("kms", Some(ext.clone())))));
        let secrets = Arc::new(Secrets::new(ctx("secretsmanager", Some(ext))));
        Self {
            secrets,
            h: Harness::new(Secrets::new(ctx("unused", None))),
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
        let out = self.rt.block_on(self.secrets.call(call))?;
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

    fn value(&self, region: &str, id: &str) -> Value {
        self.ok(region, "GetSecretValue", &json!({"SecretId": id}))["SecretString"].clone()
    }
}

#[test]
fn a_replica_follows_its_primary_and_is_read_only() {
    let r = Regions::new();
    let made = r.ok(
        EAST,
        "CreateSecret",
        &json!({"Name": "db/pass", "SecretString": "one",
        "AddReplicaRegions": [{"Region": WEST}]}),
    );
    assert_eq!(made["ReplicationStatus"][0]["Region"], WEST);
    let arn = made["ARN"].as_str().unwrap().to_owned();
    let d = r.ok(EAST, "DescribeSecret", &json!({"SecretId": "db/pass"}));
    assert_eq!(d["PrimaryRegion"], EAST);
    assert_eq!(d["ReplicationStatus"][0]["Status"], "InSync");
    assert_eq!(r.value(WEST, "db/pass"), "one");
    let replica = r.ok(WEST, "DescribeSecret", &json!({"SecretId": "db/pass"}));
    assert_eq!(replica["ARN"], arn.replace(EAST, WEST));
    assert_eq!(replica["PrimaryRegion"], EAST);
    assert!(replica.get("ReplicationStatus").is_none());
    r.ok(
        EAST,
        "PutSecretValue",
        &json!({"SecretId": "db/pass", "SecretString": "two"}),
    );
    r.ok(
        EAST,
        "TagResource",
        &json!({"SecretId": "db/pass", "Tags": [{"Key": "k", "Value": "v"}]}),
    );
    assert_eq!(r.value(WEST, "db/pass"), "two");
    let replica = r.ok(
        WEST,
        "DescribeSecret",
        &json!({"SecretId": arn.replace(EAST, WEST)}),
    );
    assert_eq!(replica["Tags"], json!([{"Key": "k", "Value": "v"}]));
    let previous = r.ok(
        WEST,
        "GetSecretValue",
        &json!({"SecretId": "db/pass", "VersionStage": "AWSPREVIOUS"}),
    );
    assert_eq!(previous["SecretString"], "one");
    for (op, body) in [
        (
            "PutSecretValue",
            json!({"SecretId": "db/pass", "SecretString": "x"}),
        ),
        (
            "UpdateSecret",
            json!({"SecretId": "db/pass", "Description": "x"}),
        ),
        ("DeleteSecret", json!({"SecretId": "db/pass"})),
        (
            "TagResource",
            json!({"SecretId": "db/pass", "Tags": [{"Key": "a", "Value": "b"}]}),
        ),
    ] {
        assert_eq!(
            r.code(WEST, op, &body),
            "InvalidParameterException",
            "{op} on a replica"
        );
    }
    assert_eq!(
        r.code(EAST, "DeleteSecret", &json!({"SecretId": "db/pass"})),
        "InvalidParameterException"
    );
}

#[test]
fn a_replicated_secret_is_listed_by_its_primary_region() {
    let r = Regions::new();
    r.ok(
        EAST,
        "CreateSecret",
        &json!({"Name": "one", "SecretString": "v", "AddReplicaRegions": [{"Region": WEST}]}),
    );
    r.ok(
        EAST,
        "CreateSecret",
        &json!({"Name": "two", "SecretString": "v"}),
    );
    let filter = json!({"Filters": [{"Key": "primary-region", "Values": [EAST]}]});
    let east = r.ok(EAST, "ListSecrets", &filter);
    assert_eq!(east["SecretList"].as_array().unwrap().len(), 1);
    assert_eq!(east["SecretList"][0]["PrimaryRegion"], EAST);
    let west = r.ok(WEST, "ListSecrets", &filter);
    assert_eq!(west["SecretList"][0]["Name"], "one");
}

#[test]
fn replicas_are_added_removed_and_promoted() {
    let r = Regions::new();
    r.ok(
        EAST,
        "CreateSecret",
        &json!({"Name": "app", "SecretString": "v"}),
    );
    let added = r.ok(EAST, "ReplicateSecretToRegions", &json!({"SecretId": "app",
        "AddReplicaRegions": [{"Region": WEST}, {"Region": EU, "KmsKeyId": "alias/aws/secretsmanager"}]}));
    assert_eq!(added["ReplicationStatus"].as_array().unwrap().len(), 2);
    // Asked again for a region it is in: made again, as AWS does (recorded), not refused; its own region is refused.
    let again = r.ok(
        EAST,
        "ReplicateSecretToRegions",
        &json!({"SecretId": "app", "AddReplicaRegions": [{"Region": WEST}]}),
    );
    assert_eq!(again["ReplicationStatus"].as_array().unwrap().len(), 2);
    assert_eq!(
        r.code(
            EAST,
            "ReplicateSecretToRegions",
            &json!({"SecretId": "app", "AddReplicaRegions": [{"Region": EAST}]})
        ),
        "InvalidParameterException"
    );
    assert_eq!(r.value(EU, "app"), "v");
    let removed = r.ok(
        EAST,
        "RemoveRegionsFromReplication",
        &json!({"SecretId": "app", "RemoveReplicaRegions": [EU]}),
    );
    assert_eq!(removed["ReplicationStatus"][0]["Region"], WEST);
    assert_eq!(
        r.code(EU, "GetSecretValue", &json!({"SecretId": "app"})),
        "ResourceNotFoundException"
    );
    assert_eq!(
        r.code(
            EAST,
            "StopReplicationToReplica",
            &json!({"SecretId": "app"})
        ),
        "InvalidParameterException"
    );
    r.ok(
        WEST,
        "StopReplicationToReplica",
        &json!({"SecretId": "app"}),
    );
    r.ok(
        WEST,
        "PutSecretValue",
        &json!({"SecretId": "app", "SecretString": "west's own"}),
    );
    let primary = r.ok(EAST, "DescribeSecret", &json!({"SecretId": "app"}));
    assert!(primary.get("ReplicationStatus").is_none());
    assert!(primary.get("PrimaryRegion").is_none());
    r.ok(
        EAST,
        "PutSecretValue",
        &json!({"SecretId": "app", "SecretString": "east"}),
    );
    assert_eq!(r.value(WEST, "app"), "west's own");
    r.ok(
        EAST,
        "DeleteSecret",
        &json!({"SecretId": "app", "ForceDeleteWithoutRecovery": true}),
    );
}

#[test]
fn a_secret_of_the_same_name_fails_replication_unless_overwritten() {
    let r = Regions::new();
    r.ok(
        WEST,
        "CreateSecret",
        &json!({"Name": "taken", "SecretString": "west"}),
    );
    r.ok(
        EAST,
        "CreateSecret",
        &json!({"Name": "taken", "SecretString": "east"}),
    );
    r.ok(
        EAST,
        "ReplicateSecretToRegions",
        &json!({"SecretId": "taken", "AddReplicaRegions": [{"Region": WEST}]}),
    );
    let status =
        r.ok(EAST, "DescribeSecret", &json!({"SecretId": "taken"}))["ReplicationStatus"][0].clone();
    assert_eq!(status["Status"], "Failed");
    assert_eq!(
        status["StatusMessage"],
        "Secret with this name already exists in this region"
    );
    assert_eq!(r.value(WEST, "taken"), "west");
    r.ok(
        EAST,
        "RemoveRegionsFromReplication",
        &json!({"SecretId": "taken", "RemoveReplicaRegions": [WEST]}),
    );
    assert_eq!(
        r.value(WEST, "taken"),
        "west",
        "a secret that never became a replica is not removed"
    );
    r.ok(
        EAST,
        "ReplicateSecretToRegions",
        &json!({"SecretId": "taken", "ForceOverwriteReplicaSecret": true,
        "AddReplicaRegions": [{"Region": WEST}]}),
    );
    assert_eq!(r.value(WEST, "taken"), "east");
    let status =
        r.ok(EAST, "DescribeSecret", &json!({"SecretId": "taken"}))["ReplicationStatus"][0].clone();
    assert_eq!(status["Status"], "InSync");
}
