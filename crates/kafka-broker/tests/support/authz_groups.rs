//! The harness of the group and transaction authorization tests: brokers with ACLs, the callers to ask as, and
//! `five_ways!`, which checks one API against the same five questions (the right granted, the right missing, a
//! super user against a DENY, a DENY against an ALLOW, and the other ways of granting: any user, a prefix, the
//! wildcard, and the per-resource default).
//!
//! Included by each test with `#[macro_use] #[path = "support/authz_groups.rs"] mod gx;`.
#![allow(
    unreachable_pub,
    dead_code,
    unused_macros,
    unused_imports,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::time::Duration;

pub use crate::support::acls::{ALLOW, DENY, LITERAL, PREFIXED, admin, creation, grant, user};
use crate::support::{Rig, rig_with, topic};
use kafka_broker::Caller;
use kafka_wire::StrBytes;
use kafka_wire::api::{Request, Response};
use kafka_wire::messages::{CreateTopicsRequest, GroupId, TopicName};

pub use crate::support::acls::{ALL, ALTER, CLUSTER, DESCRIBE, GROUP, READ, TOPIC, TXN_ID, WRITE};

pub const DELETE: i8 = 6;

pub const TOPIC_AUTH: i16 = 29;
pub const GROUP_AUTH: i16 = 30;
pub const TXN_AUTH: i16 = 53;

pub fn text(s: &str) -> StrBytes {
    StrBytes::from_string(s.to_owned())
}

pub fn group_id(s: &str) -> GroupId {
    GroupId(text(s))
}

pub fn topic_name(s: &str) -> TopicName {
    TopicName(text(s))
}

/// A broker whose default refuses, whose super user is `admin`, and whose groups form at once.
pub fn closed(c: &mut kafka_broker::Config) {
    crate::support::acls::closed_world(c);
    c.group_initial_rebalance_delay = Duration::ZERO;
}

/// A broker whose default allows (Kafka's `allow.everyone.if.no.acl.found`), super user `admin`.
pub fn open(c: &mut kafka_broker::Config) {
    c.super_users.insert("User:admin".to_owned());
    c.group_initial_rebalance_delay = Duration::ZERO;
}

pub fn world() -> Rig {
    rig_with(closed)
}

/// The topic `name` with `partitions` partitions, made by the super user (the broker may already have it).
pub async fn make_topic(rig: &Rig, name: &str, partitions: i32) {
    let mut req = CreateTopicsRequest::default();
    req.topics = vec![topic(name, partitions)];
    rig.ask_as(&admin(), 7, Request::CreateTopics(req)).await;
}

/// An ALLOW of `op` on the literal resource `(res, name)` to `who` (`User:alice` and so on).
pub fn allow(
    res: i8,
    name: &str,
    who: &str,
    op: i8,
) -> kafka_wire::messages::create_acls_request::AclCreation {
    creation((res, name, LITERAL), who, op, ALLOW)
}

pub fn deny(
    res: i8,
    name: &str,
    who: &str,
    op: i8,
) -> kafka_wire::messages::create_acls_request::AclCreation {
    creation((res, name, LITERAL), who, op, DENY)
}

/// One resource and operation an API needs, and what it answers when the caller may not.
#[derive(Clone, Copy)]
pub struct Need {
    pub res: i8,
    pub name: &'static str,
    pub op: i8,
    pub code: i16,
}

pub const fn need(res: i8, name: &'static str, op: i8, code: i16) -> Need {
    Need {
        res,
        name,
        op,
        code,
    }
}

pub enum Expect {
    /// Not refused (the API may well say something else about the request).
    Pass,
    /// Refused with this code.
    Denied(i16),
}

pub struct Case {
    pub who: Caller,
    pub label: String,
    pub expect: Expect,
}

pub struct Variant {
    pub label: &'static str,
    pub rig: Rig,
    pub cases: Vec<Case>,
}

impl Case {
    pub fn check(&self, variant: &str, got: i16, needs: &[Need]) {
        match self.expect {
            Expect::Pass => assert!(
                !needs.iter().any(|n| n.code == got),
                "{variant}: {} should pass, was refused with {got}",
                self.label
            ),
            Expect::Denied(code) => assert_eq!(
                got, code,
                "{variant}: {} should be refused with {code}",
                self.label
            ),
        }
    }
}

fn case(name: &str, expect: Expect) -> Case {
    Case {
        who: user(name),
        label: name.to_owned(),
        expect,
    }
}

fn principal(name: &str) -> String {
    format!("User:{name}")
}

fn allows_all(
    needs: &[Need],
    who: &str,
    except: Option<usize>,
) -> Vec<kafka_wire::messages::create_acls_request::AclCreation> {
    needs
        .iter()
        .enumerate()
        .filter(|(i, _)| Some(*i) != except)
        .map(|(_, n)| allow(n.res, n.name, &principal(who), n.op))
        .collect()
}

/// The brokers `five_ways!` asks, each with the callers to ask as and what each is to be told.
pub async fn variants(needs: &[Need]) -> Vec<Variant> {
    let first = needs[0].code;
    let mut out = Vec::new();

    // The right, named: granted, missing, against a DENY, DENY against ALLOW, and each need left out in turn.
    let rig = world();
    let mut acls = allows_all(needs, "alice", None);
    acls.extend(allows_all(needs, "carol", None));
    acls.push(deny(needs[0].res, needs[0].name, "User:carol", needs[0].op));
    // The super user is denied everything, and still passes.
    acls.extend(needs.iter().map(|n| deny(n.res, n.name, "User:admin", ALL)));
    // Somebody else's right makes the resource covered, so the default does not apply to bob.
    acls.push(allow(
        needs[0].res,
        needs[0].name,
        "User:other",
        needs[0].op,
    ));
    let mut cases = vec![
        case("alice", Expect::Pass),
        case("bob", Expect::Denied(first)),
        case("admin", Expect::Pass),
        case("carol", Expect::Denied(first)),
    ];
    for (i, n) in needs.iter().enumerate() {
        let who = format!("miss{i}");
        acls.extend(allows_all(needs, &who, Some(i)));
        cases.push(Case {
            who: user(&who),
            label: format!("{who} (everything but the {i}th)"),
            expect: Expect::Denied(n.code),
        });
    }
    grant(&rig, acls).await;
    out.push(Variant {
        label: "named",
        rig,
        cases,
    });

    // Granted to any user, to a prefix of the name, to the wildcard name.
    let rig = world();
    let mut acls = Vec::new();
    for n in needs {
        acls.push(allow(n.res, n.name, "User:*", n.op));
        acls.push(creation(
            (n.res, &n.name[..2], PREFIXED),
            "User:erin",
            n.op,
            ALLOW,
        ));
        acls.push(creation(
            (n.res, &n.name[..1], PREFIXED),
            "User:frank",
            n.op,
            ALLOW,
        ));
        acls.push(creation((n.res, "*", LITERAL), "User:grace", n.op, ALLOW));
        acls.push(creation((n.res, "zz", PREFIXED), "User:heidi", n.op, ALLOW));
    }
    grant(&rig, acls).await;
    out.push(Variant {
        label: "any user, prefix, wildcard",
        rig,
        cases: vec![
            case("dave", Expect::Pass),
            case("erin", Expect::Pass),
            case("frank", Expect::Pass),
            case("grace", Expect::Pass),
            case("admin", Expect::Pass),
        ],
    });

    // A prefix that is not the name's prefix covers nothing of it: only the default, which is closed.
    let rig = world();
    grant(
        &rig,
        needs
            .iter()
            .map(|n| creation((n.res, "zz", PREFIXED), "User:heidi", n.op, ALLOW))
            .collect(),
    )
    .await;
    out.push(Variant {
        label: "a prefix that is not the name's",
        rig,
        cases: vec![case("heidi", Expect::Denied(first))],
    });

    // The default allows while nothing covers the resource; one ACL on it for anybody turns the default off.
    let rig = rig_with(open);
    let cases = vec![case("hank", Expect::Pass)];
    out.push(Variant {
        label: "default allows, nothing covers",
        rig,
        cases,
    });
    let rig = rig_with(open);
    grant(&rig, allows_all(needs, "alice", None)).await;
    out.push(Variant {
        label: "default allows, one ACL covers",
        rig,
        cases: vec![
            case("alice", Expect::Pass),
            case("hank", Expect::Denied(first)),
        ],
    });
    out
}

/// Runs `$body` (an expression that awaits an API's answer and returns its error code) for every caller of
/// every variant, with `$rig` and `$who` bound, and checks each against what it is to be told.
macro_rules! five_ways {
    ($needs:expr, |$rig:ident, $who:ident| $body:expr) => {{
        let needs: &[$crate::gx::Need] = $needs;
        for v in $crate::gx::variants(needs).await {
            let $rig = &v.rig;
            for c in &v.cases {
                let $who = &c.who;
                let got: i16 = $body;
                c.check(v.label, got, needs);
            }
        }
    }};
}
