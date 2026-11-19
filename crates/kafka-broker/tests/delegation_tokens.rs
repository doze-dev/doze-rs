//! Delegation tokens (behaviours ADM-17, SEC-347 to SEC-350): made, described, renewed, expired, swept and kept across a
//! restart; refused without SASL, to a token's own login, and without a key. Held to real Kafka 4.4.0 by
//! `crates/kafka-wire/examples/oracle_admin` and `kafka-delegation-tokens.sh`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use std::sync::Arc;

use bytes::Bytes;
use kafka_auth::{Principal, Secret};
use kafka_broker::{Caller, Config};
use kafka_storage::fs::Fs;
use kafka_storage::sim::SimFs;
use kafka_wire::StrBytes;
use kafka_wire::api::{Request, Response};
use kafka_wire::messages::create_delegation_token_request::CreatableRenewers;
use kafka_wire::messages::describe_delegation_token_request::DescribeDelegationTokenOwner;
use kafka_wire::messages::{
    CreateDelegationTokenRequest, CreateDelegationTokenResponse, DescribeDelegationTokenRequest,
    ExpireDelegationTokenRequest, RenewDelegationTokenRequest,
};
use support::acls::{ALLOW, LITERAL, creation, grant};
use support::{Rig, open_on_with};

const DAY: i64 = 24 * 3600 * 1000;

fn keyed(c: &mut Config) {
    c.delegation_tokens.secret = Some(Arc::new(Secret::new(b"the-key".to_vec())));
    c.super_users.insert("User:admin".to_owned());
}

fn fresh(tweak: impl FnOnce(&mut Config)) -> Rig {
    let fs = SimFs::new();
    fs.create_dir(std::path::Path::new("/")).ok();
    open_on_with(fs, tweak)
}

fn sasl(name: &str) -> Caller {
    Caller::logged_in(Principal::user(name), Some("10.0.0.1".parse().unwrap()), false)
}

fn text(s: &str) -> StrBytes {
    StrBytes::from_string(s.to_owned())
}

async fn create(rig: &Rig, who: &Caller, version: i16, owner: Option<&str>, renewers: &[(&str, &str)], max: i64) -> CreateDelegationTokenResponse {
    let mut req = CreateDelegationTokenRequest::default();
    if let Some(owner) = owner {
        req.owner_principal_type = Some(text("User"));
        req.owner_principal_name = Some(text(owner));
    }
    req.max_lifetime_ms = max;
    req.renewers = (renewers.iter())
        .map(|(kind, name)| {
            let mut r = CreatableRenewers::default();
            r.principal_type = text(kind);
            r.principal_name = text(name);
            r
        })
        .collect();
    let Response::CreateDelegationToken(r) = rig.ask_as(who, version, Request::CreateDelegationToken(req)).await else {
        panic!()
    };
    r
}

async fn renew(rig: &Rig, who: &Caller, hmac: &Bytes, period: i64) -> (i16, i64) {
    let mut req = RenewDelegationTokenRequest::default();
    req.hmac = hmac.clone();
    req.renew_period_ms = period;
    let Response::RenewDelegationToken(r) = rig.ask_as(who, 2, Request::RenewDelegationToken(req)).await else {
        panic!()
    };
    (r.error_code, r.expiry_timestamp_ms)
}

async fn expire(rig: &Rig, who: &Caller, hmac: &Bytes, period: i64) -> (i16, i64) {
    let mut req = ExpireDelegationTokenRequest::default();
    req.hmac = hmac.clone();
    req.expiry_time_period_ms = period;
    let Response::ExpireDelegationToken(r) = rig.ask_as(who, 2, Request::ExpireDelegationToken(req)).await else {
        panic!()
    };
    (r.error_code, r.expiry_timestamp_ms)
}

/// (error, token ids seen) of a DescribeDelegationToken by `who`.
async fn describe(rig: &Rig, who: &Caller, owners: Option<&[&str]>) -> (i16, Vec<String>) {
    let mut req = DescribeDelegationTokenRequest::default();
    req.owners = owners.map(|o| {
        o.iter()
            .map(|name| {
                let mut w = DescribeDelegationTokenOwner::default();
                w.principal_type = text("User");
                w.principal_name = text(name);
                w
            })
            .collect()
    });
    let Response::DescribeDelegationToken(r) = rig.ask_as(who, 3, Request::DescribeDelegationToken(req)).await else {
        panic!()
    };
    let mut ids: Vec<String> = r.tokens.iter().map(|t| t.token_id.to_string()).collect();
    ids.sort();
    (r.error_code, ids)
}

#[tokio::test]
async fn a_token_is_made_with_kafkas_times_and_an_hmac_of_its_id_under_the_key() {
    let (rig, users) = keyed_on(new_fs());
    let alice = sasl("alice");
    let r = create(&rig, &alice, 3, None, &[("User", "bob")], 3_600_000).await;
    assert_eq!(r.error_code, 0);
    assert_eq!((r.principal_name.as_str(), r.token_requester_principal_name.as_str()), ("alice", "alice"));
    assert_eq!(r.token_id.len(), 22);
    assert!(!r.token_id.contains('-'));
    assert_eq!(r.max_timestamp_ms - r.issue_timestamp_ms, 3_600_000);
    assert_eq!(r.expiry_timestamp_ms, r.max_timestamp_ms, "the expiry is never past the maximum");
    assert_eq!(&r.hmac[..], &kafka_auth::tokens::hmac(b"the-key", &r.token_id)[..]);
    // No maximum asked: Kafka's seven days, and a day to the expiry.
    let r = create(&rig, &alice, 1, None, &[], 0).await;
    assert_eq!((r.max_timestamp_ms - r.issue_timestamp_ms, r.expiry_timestamp_ms - r.issue_timestamp_ms), (7 * DAY, DAY));
    // The token logs in, as its owner.
    let login = users.token(&r.token_id).unwrap();
    assert_eq!(login.owner().as_str(), "User:alice");
}

#[tokio::test]
async fn token_requests_are_refused_before_anything_without_sasl_or_with_a_token_or_without_a_key() {
    let rig = fresh(keyed);
    let bogus = Bytes::from_static(&[7; 64]);
    for who in [support::caller(), Caller::logged_in(Principal::user("alice"), None, true)] {
        let r = create(&rig, &who, 3, None, &[("Group", "g")], 0).await;
        assert_eq!((r.error_code, r.issue_timestamp_ms, r.token_id.as_str()), (64, -1, ""));
        assert_eq!(renew(&rig, &who, &bogus, 0).await, (64, -1));
        assert_eq!(expire(&rig, &who, &bogus, 0).await, (64, -1));
        assert_eq!(describe(&rig, &who, None).await.0, 64);
    }
    let off = fresh(|_| {});
    let alice = sasl("alice");
    let r = create(&off, &alice, 3, None, &[], 0).await;
    assert_eq!((r.error_code, r.issue_timestamp_ms, r.principal_name.as_str()), (61, 0, "alice"));
    assert_eq!(renew(&off, &alice, &bogus, 0).await, (61, 0));
    assert_eq!(expire(&off, &alice, &bogus, 0).await, (61, 0));
    assert_eq!(describe(&off, &alice, None).await, (61, vec![]));
    // A renewer that is not a user, then with a key the owner's right on another user.
    assert_eq!(create(&rig, &alice, 3, None, &[("Group", "g")], 0).await.error_code, 67);
}

#[tokio::test]
async fn owners_renewers_and_the_rights_decide_who_renews_expires_and_sees_a_token() {
    // A closed world: with everything allowed that no ACL covers, every login may see every token, as in Kafka.
    let users = Arc::new(kafka_auth::Users::none());
    let shared = Arc::clone(&users);
    let rig = open_on_with(new_fs(), move |c| {
        keyed(c);
        c.users = shared;
        c.acl_default_allow = false;
    });
    let (alice, bob, carol, admin) = (sasl("alice"), sasl("bob"), sasl("carol"), sasl("admin"));
    let mine = create(&rig, &alice, 3, None, &[("User", "bob")], 0).await;
    let hmac = mine.hmac.clone();
    let id = mine.token_id.to_string();
    // For another user: CREATE_TOKENS on that user.
    let r = create(&rig, &alice, 3, Some("dave"), &[], 0).await;
    assert_eq!((r.error_code, r.principal_name.as_str(), r.token_requester_principal_name.as_str()), (65, "dave", "alice"));
    grant(&rig, vec![creation((7, "User:dave", LITERAL), "User:alice", 13, ALLOW)]).await;
    let r = create(&rig, &alice, 3, Some("dave"), &[], 0).await;
    assert_eq!((r.error_code, r.principal_name.as_str()), (0, "dave"));
    // The requester sees the token it asked for; dave's other rights do not matter.
    assert!(describe(&rig, &alice, None).await.1.contains(&r.token_id.to_string()));
    // Seen by the owner, a renewer and a super user; carol sees none of alice's; an empty owner list is nothing.
    assert!(describe(&rig, &alice, None).await.1.contains(&id));
    assert!(describe(&rig, &bob, None).await.1.contains(&id));
    assert!(describe(&rig, &admin, None).await.1.contains(&id));
    assert!(!describe(&rig, &carol, None).await.1.contains(&id));
    assert_eq!(describe(&rig, &alice, Some(&[])).await, (0, vec![]));
    assert_eq!(describe(&rig, &alice, Some(&["bob"])).await.1, vec![id.clone()]);
    // DESCRIBE on the token lets carol see it.
    grant(&rig, vec![creation((6, &id, LITERAL), "User:carol", 8, ALLOW)]).await;
    assert!(describe(&rig, &carol, None).await.1.contains(&id));
    // Renewals: the owner and a renewer, not anybody else; a minute asked is a minute.
    assert_eq!(renew(&rig, &admin, &hmac, 0).await.0, 63);
    assert_eq!(renew(&rig, &alice, &Bytes::from_static(&[1; 64]), 0).await.0, 62);
    let (code, expiry) = renew(&rig, &bob, &hmac, 60_000).await;
    assert_eq!(code, 0);
    assert!((expiry - kafka_now() - 60_000).abs() < 5_000);
    // Expiry: now, then the token is gone.
    assert_eq!(expire(&rig, &carol, &hmac, -1).await.0, 63);
    assert_eq!(expire(&rig, &alice, &hmac, -1).await.0, 0);
    assert_eq!(expire(&rig, &alice, &hmac, -1).await.0, 62);
    assert!(!describe(&rig, &alice, None).await.1.contains(&id));
    assert!(users.token(&id).is_none());
}

#[tokio::test]
async fn an_expired_token_lives_until_the_sweep_and_tokens_outlive_a_restart() {
    let fs = new_fs();
    let alice = sasl("alice");
    let (kept, gone) = {
        let (rig, _) = keyed_on(fs.clone());
        let kept = create(&rig, &alice, 3, None, &[], 0).await;
        let gone = create(&rig, &alice, 3, None, &[], 0).await;
        // An expiry of now: expired a moment later, and still there until the sweep.
        assert_eq!(expire(&rig, &alice, &gone.hmac, 0).await.0, 0);
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        assert_eq!(renew(&rig, &alice, &gone.hmac, 0).await.0, 66);
        assert_eq!(expire(&rig, &alice, &gone.hmac, 10).await.0, 66);
        assert_eq!(describe(&rig, &alice, None).await.1.len(), 2);
        rig.broker.sweep_expired_tokens().await;
        assert_eq!(describe(&rig, &alice, None).await.1, vec![kept.token_id.to_string()]);
        rig.broker.shutdown().await.unwrap();
        (kept, gone)
    };
    let (rig, users) = keyed_on(fs);
    assert_eq!(describe(&rig, &alice, None).await.1, vec![kept.token_id.to_string()]);
    assert!(users.token(&kept.token_id).is_some());
    assert!(users.token(&gone.token_id).is_none());
    assert_eq!(renew(&rig, &alice, &kept.hmac, 0).await.0, 0);
}

fn kafka_now() -> i64 {
    i64::try_from(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis()).unwrap()
}

/// A keyed broker on `fs`, and the users its logins read (the server's authenticator shares them).
fn keyed_on(fs: SimFs) -> (Rig, Arc<kafka_auth::Users>) {
    let users = Arc::new(kafka_auth::Users::none());
    let shared = Arc::clone(&users);
    let rig = open_on_with(fs, move |c| {
        keyed(c);
        c.users = shared;
    });
    (rig, users)
}

fn new_fs() -> SimFs {
    let fs = SimFs::new();
    fs.create_dir(std::path::Path::new("/")).ok();
    fs
}
