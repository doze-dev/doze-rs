//! Slow cryptography leaves the actor and the event loop (`offload.rs`): the actor hands back a job or a deferred answer, never
//! doing the work itself, and other calls are answered while it runs.

use std::sync::Arc;
use std::time::{Duration, Instant};

use aws_testkit::Harness;
use doze_runtime::{Ctx, SystemClock, Tasks};
use serde_json::{Value, json};

use super::World;
use crate::Kms;
use crate::offload::Pending;

/// Dispatches `op` on the core alone (no inline work) and answers what it left pending.
fn dispatch_raw(w: &mut World, op: &str, request: &Value) -> Option<Pending> {
    let call = w.h.call(w.h.op(op), request).unwrap();
    let reply = w.core.dispatch(&call);
    w.core.pending(reply.is_ok())
}

#[test]
fn the_actor_leaves_key_making_and_private_key_work_to_a_job() {
    let mut w = World::new();
    let id = w.key(&json!({"KeySpec": "RSA_2048", "KeyUsage": "SIGN_VERIFY"}));
    let sign =
        json!({"KeyId": id, "Message": "aGVsbG8=", "SigningAlgorithm": "RSASSA_PSS_SHA_256"});
    // The first use makes the key: the actor asks for it to be made and does not make it.
    assert!(matches!(
        dispatch_raw(&mut w, "Sign", &sign),
        Some(Pending::Job(_))
    ));
    assert!(
        w.core
            .keys
            .get(&id)
            .unwrap()
            .current()
            .unwrap()
            .sealed
            .is_empty()
    );
    // Once made, the signature itself is left to be made off the actor.
    w.ok("GetPublicKey", &json!({"KeyId": id}));
    assert!(matches!(
        dispatch_raw(&mut w, "Sign", &sign),
        Some(Pending::Answer(_))
    ));
    let signature = w.ok("Sign", &sign)["Signature"].clone();
    let mut verify = sign.clone();
    verify["Signature"] = signature;
    assert!(matches!(
        dispatch_raw(&mut w, "Verify", &verify),
        Some(Pending::Answer(_))
    ));
    assert_eq!(w.ok("Verify", &verify)["SignatureValid"], true);

    let sym = w.key(&json!({}));
    let pair = json!({"KeyId": sym, "KeyPairSpec": "RSA_2048"});
    assert!(matches!(
        dispatch_raw(&mut w, "GenerateDataKeyPair", &pair),
        Some(Pending::Job(_))
    ));
    // A pair for a key that is not there makes nothing.
    let missing = json!({"KeyId": "alias/none", "KeyPairSpec": "RSA_4096"});
    assert!(dispatch_raw(&mut w, "GenerateDataKeyPair", &missing).is_none());

    let external = w.key(&json!({"Origin": "EXTERNAL"}));
    let params = json!({"KeyId": external, "WrappingAlgorithm": "RSAES_OAEP_SHA_256", "WrappingKeySpec": "RSA_2048"});
    assert!(matches!(
        dispatch_raw(&mut w, "GetParametersForImport", &params),
        Some(Pending::Job(_))
    ));
}

#[test]
fn a_failed_check_off_the_actor_is_not_a_use_of_the_key() {
    let mut w = World::new();
    let id = w.key(&json!({"KeySpec": "ECC_NIST_P256", "KeyUsage": "SIGN_VERIFY"}));
    let bad = json!({"KeyId": id, "Message": "aGVsbG8=", "SigningAlgorithm": "ECDSA_SHA_256", "Signature": "MAYCAQECAQE="});
    assert_eq!(w.err("Verify", &bad).0, "KMSInvalidSignatureException");
    assert!(!w.core.used.contains_key(&id));
    w.ok(
        "Sign",
        &json!({"KeyId": id, "Message": "aGVsbG8=", "SigningAlgorithm": "ECDSA_SHA_256"}),
    );
    assert_eq!(w.core.used.get(&id).unwrap().operation, "Sign");
}

/// Before: the first Sign of an RSA key made the key inside the actor, on the event loop, and a call asked meanwhile waited for it.
/// After: the key is made on a blocking thread, and the call asked meanwhile is answered first.
#[tokio::test]
async fn a_call_is_answered_while_a_key_is_made() {
    let dir = super::temp();
    let ctx = || {
        Arc::new(Ctx {
            dir: dir.path().to_path_buf(),
            idle: Duration::from_secs(300),
            tasks: Tasks::new(),
            clock: Arc::new(SystemClock),
            ext: None,
        })
    };
    let kms = Kms::new(ctx());
    let h = Harness::new(Kms::new(ctx()));
    let ask = |op: &str, request: Value| {
        aws_wire::service::Service::call(&kms, h.call(h.op(op), &request).unwrap())
    };
    let made = ask(
        "CreateKey",
        json!({"KeySpec": "RSA_2048", "KeyUsage": "SIGN_VERIFY"}),
    )
    .await
    .unwrap();
    let id = h.json(h.op("CreateKey"), &made)["KeyMetadata"]["KeyId"]
        .as_str()
        .unwrap()
        .to_owned();
    let start = Instant::now();
    let sign = async {
        ask(
            "Sign",
            json!({"KeyId": id, "Message": "aGVsbG8=", "SigningAlgorithm": "RSASSA_PSS_SHA_256"}),
        )
        .await
        .unwrap();
        start.elapsed()
    };
    // Asked 5 ms in: on a held loop even the timer fires late, so the wait is counted from when it was due.
    let list = async {
        tokio::time::sleep(Duration::from_millis(5)).await;
        ask("ListKeys", json!({})).await.unwrap();
        start.elapsed().saturating_sub(Duration::from_millis(5))
    };
    let (signed, listed) = tokio::join!(sign, list);
    assert!(
        listed * 2 < signed,
        "ListKeys waited {listed:?} while the key was made (the Sign took {signed:?})"
    );
}
