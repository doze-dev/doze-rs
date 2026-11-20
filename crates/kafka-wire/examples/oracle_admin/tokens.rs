//! The delegation-token steps: made, described, renewed and expired by their owner, a renewer and others, and refused
//! without SASL. Ids and HMACs differ by nature; times are kept as differences.

use bytes::Bytes;
use kafka_protocol::messages::create_acls_request::AclCreation;
use kafka_protocol::messages::create_delegation_token_request::CreatableRenewers;
use kafka_protocol::messages::describe_delegation_token_request::DescribeDelegationTokenOwner;
use kafka_protocol::messages::{
    ApiKey, CreateAclsRequest, CreateAclsResponse, CreateDelegationTokenRequest,
    CreateDelegationTokenResponse, DescribeDelegationTokenRequest, DescribeDelegationTokenResponse,
    ExpireDelegationTokenRequest, ExpireDelegationTokenResponse, RenewDelegationTokenRequest,
    RenewDelegationTokenResponse,
};

use crate::conn::{Pair, Tally};
use crate::steps::s;

const HOUR: i64 = 3_600_000;

fn now_ms() -> i64 {
    i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap()
}

/// A time as how far it is from now, to the nearest ten seconds (and -1 and 0 as they are).
fn from_now(ms: i64) -> i64 {
    if ms <= 0 {
        return ms;
    }
    ((ms - now_ms()) as f64 / 10_000.0).round() as i64 * 10_000
}

fn create(
    owner: Option<(&str, &str)>,
    renewers: &[(&str, &str)],
    max_ms: i64,
) -> CreateDelegationTokenRequest {
    let mut req = CreateDelegationTokenRequest::default();
    if let Some((kind, name)) = owner {
        req.owner_principal_type = Some(s(kind));
        req.owner_principal_name = Some(s(name));
    }
    req.max_lifetime_ms = max_ms;
    req.renewers = (renewers.iter())
        .map(|(kind, name)| {
            let mut r = CreatableRenewers::default();
            r.principal_type = s(kind);
            r.principal_name = s(name);
            r
        })
        .collect();
    req
}

fn norm_created(r: &mut CreateDelegationTokenResponse) {
    if r.issue_timestamp_ms > 0 {
        r.expiry_timestamp_ms -= r.issue_timestamp_ms;
        r.max_timestamp_ms -= r.issue_timestamp_ms;
        r.issue_timestamp_ms = from_now(r.issue_timestamp_ms);
    }
    if !r.token_id.is_empty() {
        assert_eq!(r.token_id.len(), 22, "a token id is a Kafka Uuid");
        assert!(!r.token_id.contains('-'));
        r.token_id = s("<id>");
    }
    r.hmac = Bytes::from(format!("<{} bytes>", r.hmac.len()).into_bytes());
}

fn describe(owners: Option<&[&str]>) -> DescribeDelegationTokenRequest {
    let mut req = DescribeDelegationTokenRequest::default();
    req.owners = owners.map(|o| {
        o.iter()
            .map(|name| {
                let mut w = DescribeDelegationTokenOwner::default();
                w.principal_type = s("User");
                w.principal_name = s(name);
                w
            })
            .collect()
    });
    req
}

fn norm_described(r: &mut DescribeDelegationTokenResponse) {
    for t in &mut r.tokens {
        t.expiry_timestamp -= t.issue_timestamp;
        t.max_timestamp -= t.issue_timestamp;
        t.issue_timestamp = from_now(t.issue_timestamp);
        t.token_id = s("<id>");
        t.hmac = Bytes::from(format!("<{} bytes>", t.hmac.len()).into_bytes());
    }
    r.tokens.sort_by_key(|t| format!("{t:?}"));
}

fn renew(hmac: &Bytes, period: i64) -> RenewDelegationTokenRequest {
    let mut req = RenewDelegationTokenRequest::default();
    req.hmac = hmac.clone();
    req.renew_period_ms = period;
    req
}

fn expire(hmac: &Bytes, period: i64) -> ExpireDelegationTokenRequest {
    let mut req = ExpireDelegationTokenRequest::default();
    req.hmac = hmac.clone();
    req.expiry_time_period_ms = period;
    req
}

fn norm_renewed(r: &mut RenewDelegationTokenResponse) {
    r.expiry_timestamp_ms = from_now(r.expiry_timestamp_ms);
}

fn norm_expired(r: &mut ExpireDelegationTokenResponse) {
    r.expiry_timestamp_ms = from_now(r.expiry_timestamp_ms);
}

pub(crate) fn tokens(as_user: &dyn Fn(&str) -> Pair, t: &mut Tally) {
    let mut alice = as_user("alice");
    let mut bob = as_user("bob");
    let mut admin = as_user("admin");
    let ck = ApiKey::CreateDelegationToken;
    let mut made = None;
    for v in 1..=3 {
        let (k, d) = alice.same(
            t,
            "alice's own, bob renews",
            ck,
            v,
            &create(None, &[("User", "bob")], HOUR),
            norm_created,
        );
        made = Some((k.hmac, d.hmac));
    }
    let (kh, dh) = made.unwrap();
    alice.same(
        t,
        "a lifetime past the most",
        ck,
        3,
        &create(None, &[], 30 * 24 * HOUR),
        norm_created,
    );
    alice.same(
        t,
        "a renewer not a user",
        ck,
        3,
        &create(None, &[("Group", "g")], 0),
        norm_created,
    );
    alice.same(
        t,
        "for carol, nothing in the way",
        ck,
        3,
        &create(Some(("User", "carol")), &[], 0),
        norm_created,
    );
    // An ACL on carol's user resource that does not name alice: she may no longer make tokens for carol.
    let mut acls = CreateAclsRequest::default();
    let mut a = AclCreation::default();
    a.resource_type = 7;
    a.resource_name = s("User:carol");
    a.resource_pattern_type = 3;
    a.principal = s("User:admin");
    a.host = s("*");
    a.operation = 13;
    a.permission_type = 3;
    acls.creations.push(a);
    let _: (CreateAclsResponse, CreateAclsResponse) = admin.both(ApiKey::CreateAcls, 3, &acls);
    std::thread::sleep(std::time::Duration::from_secs(2));
    alice.same(
        t,
        "for carol, refused",
        ck,
        3,
        &create(Some(("User", "carol")), &[], 0),
        norm_created,
    );

    let dk = ApiKey::DescribeDelegationToken;
    for v in 1..=3 {
        alice.same(
            t,
            "alice: every token",
            dk,
            v,
            &describe(None),
            norm_described,
        );
    }
    alice.same(
        t,
        "alice: no owner",
        dk,
        3,
        &describe(Some(&[])),
        norm_described,
    );
    alice.same(
        t,
        "alice: bob's",
        dk,
        3,
        &describe(Some(&["bob"])),
        norm_described,
    );
    alice.same(
        t,
        "alice: carol's",
        dk,
        3,
        &describe(Some(&["carol"])),
        norm_described,
    );
    bob.same(
        t,
        "bob, a renewer: every token",
        dk,
        3,
        &describe(None),
        norm_described,
    );
    admin.same(
        t,
        "admin, a super user: every token",
        dk,
        3,
        &describe(None),
        norm_described,
    );

    let rk = ApiKey::RenewDelegationToken;
    alice.same2(
        t,
        "the owner renews",
        rk,
        2,
        (&renew(&kh, 0), &renew(&dh, 0)),
        norm_renewed,
        None,
    );
    bob.same2(
        t,
        "a renewer renews for a minute",
        rk,
        2,
        (&renew(&kh, 60_000), &renew(&dh, 60_000)),
        norm_renewed,
        None,
    );
    admin.same2(
        t,
        "another may not",
        rk,
        2,
        (&renew(&kh, 0), &renew(&dh, 0)),
        norm_renewed,
        None,
    );
    let bogus = Bytes::from_static(&[7; 64]);
    alice.same(t, "an unknown HMAC", rk, 2, &renew(&bogus, 0), norm_renewed);
    let ek = ApiKey::ExpireDelegationToken;
    admin.same2(
        t,
        "another may not expire it",
        ek,
        2,
        (&expire(&kh, 0), &expire(&dh, 0)),
        norm_expired,
        None,
    );
    bob.same2(
        t,
        "a renewer moves its expiry",
        ek,
        2,
        (&expire(&kh, 120_000), &expire(&dh, 120_000)),
        norm_expired,
        None,
    );
    alice.same2(
        t,
        "expired at once",
        ek,
        2,
        (&expire(&kh, -1), &expire(&dh, -1)),
        norm_expired,
        None,
    );
    alice.same2(
        t,
        "and then not found",
        ek,
        2,
        (&expire(&kh, -1), &expire(&dh, -1)),
        norm_expired,
        None,
    );
    alice.same2(
        t,
        "nor renewed",
        rk,
        2,
        (&renew(&kh, 0), &renew(&dh, 0)),
        norm_renewed,
        None,
    );
    alice.same(
        t,
        "alice after one is gone",
        dk,
        3,
        &describe(None),
        norm_described,
    );
    for v in 1..=1 {
        alice.same(
            t,
            "an unknown HMAC (old)",
            ek,
            v,
            &expire(&bogus, 0),
            norm_expired,
        );
        alice.same(
            t,
            "an unknown HMAC (old)",
            rk,
            v,
            &renew(&bogus, 0),
            norm_renewed,
        );
    }
}

/// On a listener without SASL every token API is DELEGATION_TOKEN_REQUEST_NOT_ALLOWED, first of all.
pub(crate) fn not_over_plaintext(p: &mut Pair, t: &mut Tally) {
    let bogus = Bytes::from_static(&[7; 64]);
    for v in 1..=3 {
        p.same(
            t,
            "plaintext: create",
            ApiKey::CreateDelegationToken,
            v,
            &create(None, &[("Group", "g")], 0),
            norm_created,
        );
        p.same(
            t,
            "plaintext: describe",
            ApiKey::DescribeDelegationToken,
            v,
            &describe(None),
            norm_described,
        );
    }
    for v in 1..=2 {
        p.same(
            t,
            "plaintext: renew",
            ApiKey::RenewDelegationToken,
            v,
            &renew(&bogus, 0),
            norm_renewed,
        );
        p.same(
            t,
            "plaintext: expire",
            ApiKey::ExpireDelegationToken,
            v,
            &expire(&bogus, 0),
            norm_expired,
        );
    }
}
