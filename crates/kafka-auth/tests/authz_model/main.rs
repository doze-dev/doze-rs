//! The authorizer against a transcription of Kafka's own.
//!
//! `Model` below is `StandardAuthorizerData.authorize`, `findAclRule`, `checkSection`, `findResult` and
//! `MatchingRuleBuilder` of Apache Kafka 4.3.1 (metadata/.../authorizer/StandardAuthorizerData.java), with the
//! ACLs kept in the order of `StandardAcl.compareTo` (resource type, then the resource name in REVERSE, then the
//! pattern type and the rest) and walked as Kafka walks its `NavigableSet`: from an exemplar, forwards, jumping
//! to the exemplar of a shorter prefix at each name that is no prefix. `model_by_type` is
//! `Authorizer.authorizeByResourceType`. The real implementation indexes by name and scans: nothing is shared
//! with the model but the `Acl` type, so a rule written differently in the two shows as a disagreement.
//!
//! The model also reproduces a quirk of Kafka's walk that a plain reading of the rules misses: the LITERAL `*` ACL does
//! not cover a resource whose name starts with an asterisk (see `rules.rs`).
//!
//! Thousands of random ACL sets and requests, from a seeded generator (a failing case prints its seed).
//! Names use only the characters a, b, c and `*`: Kafka's walk throws for a resource whose name starts with a
//! character below `*` and shares two characters with an ACL name (`substring(0, matchesUpTo)` of the one
//! character `*`), so such names are left out of the comparison.

// The model keeps the shape of the Java it transcribes (one long function each), so that it can be read beside it.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::too_many_lines
)]

mod model;

use std::collections::BTreeSet;

use kafka_auth::authz::{
    Acl, AclSet, Resource, Rules, Subject, operation, pattern, permission, resource,
};
use model::Model;

// ---- the generator -----------------------------------------------------------------------------------------

/// xorshift64*: a failing case replays from its seed.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn pick<'a, T>(&mut self, from: &'a [T]) -> &'a T {
        &from[self.below(from.len())]
    }

    fn name(&mut self, alphabet: &[char], most: usize, least: usize) -> String {
        let len = least + self.below(most - least + 1);
        (0..len).map(|_| *self.pick(alphabet)).collect()
    }
}

const PRINCIPALS: [&str; 5] = ["User:a", "User:b", "User:*", "Group:g", "Group:*"];
const CALLERS: [&str; 4] = ["User:a", "User:b", "User:c", "Group:g"];
const ACL_HOSTS: [&str; 3] = ["*", "1.1.1.1", "2.2.2.2"];
const CALLER_HOSTS: [&str; 3] = ["1.1.1.1", "2.2.2.2", "3.3.3.3"];

fn random_acl(rng: &mut Rng, alphabet: &[char]) -> Acl {
    let kind = match rng.below(10) {
        0..=4 => resource::TOPIC,
        5..=6 => resource::GROUP,
        7 => resource::CLUSTER,
        8 => resource::TRANSACTIONAL_ID,
        _ => resource::USER,
    };
    let name = if rng.below(6) == 0 {
        "*".to_owned()
    } else {
        rng.name(alphabet, 3, 1)
    };
    Acl {
        resource_type: kind,
        name,
        pattern_type: if rng.below(2) == 0 {
            pattern::LITERAL
        } else {
            pattern::PREFIXED
        },
        principal: (*rng.pick(&PRINCIPALS)).to_owned(),
        host: (*rng.pick(&ACL_HOSTS)).to_owned(),
        // ALL is common enough to meet the DENY ALL rule; the rest are spread over the operations.
        operation: if rng.below(5) == 0 {
            operation::ALL
        } else {
            operation::READ + rng.below(13) as i8
        },
        permission: if rng.below(3) == 0 {
            permission::DENY
        } else {
            permission::ALLOW
        },
    }
}

fn check_one_set(seed: u64) {
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    // Half the sets draw from two characters, so that names collide often.
    let alphabet: &[char] = if rng.below(2) == 0 {
        &['a', 'b']
    } else {
        &['a', 'b', 'c', '*']
    };
    let count = rng.below(14);
    let acls: Vec<Acl> = (0..count).map(|_| random_acl(&mut rng, alphabet)).collect();
    let supers: &[&str] = if rng.below(4) == 0 { &["User:b"] } else { &[] };
    let default_allow = rng.below(2) == 0;
    let model = Model::new(&acls, supers, default_allow);
    let rules = Rules {
        default_allow,
        super_users: supers.iter().map(|s| (*s).to_owned()).collect(),
    };
    let set = AclSet::new(acls.iter().cloned());
    for _ in 0..30 {
        let (principal, host) = (*rng.pick(&CALLERS), *rng.pick(&CALLER_HOSTS));
        let kind = resource::TOPIC + rng.below(6) as i8;
        let name = rng.name(alphabet, 4, 0);
        let op = operation::ALL + rng.below(14) as i8;
        let who = Subject { principal, host };
        let want = model.authorize(principal, host, kind, &name, op);
        let got = rules.authorize(&set, who, Resource { kind, name: &name }, op);
        assert_eq!(
            got, want,
            "seed {seed}: {principal} from {host} doing {op} on {kind}:{name:?} with default {default_allow}, \
             supers {supers:?}, acls {acls:#?}"
        );
        let want = model.authorize_by_type(principal, host, op, kind);
        let got = rules.authorize_by_resource_type(&set, who, op, kind);
        assert_eq!(
            got, want,
            "seed {seed}: {principal} from {host} doing {op} on some {kind}, default {default_allow}, \
             supers {supers:?}, acls {acls:#?}"
        );
    }
}

fn seeds() -> u64 {
    std::env::var("AUTHZ_MODEL_SEEDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(6_000)
}

#[test]
fn the_authorizer_agrees_with_kafkas_algorithm_on_random_acl_sets() {
    for seed in 1..=seeds() {
        check_one_set(seed);
    }
}

/// The model is only worth something if the sets it is run on reach every branch: count what the random sets
/// produce, so that a generator change cannot quietly leave a rule unvisited.
#[test]
fn the_random_sets_reach_every_branch_of_the_decision() {
    let (mut deny_over_allow, mut default_used, mut covered_but_unmatched) = (0, 0, 0);
    let (mut allowed, mut denied, mut implied) = (0, 0, 0);
    for seed in 1..=2_000_u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let alphabet: &[char] = if rng.below(2) == 0 {
            &['a', 'b']
        } else {
            &['a', 'b', 'c', '*']
        };
        let count = rng.below(14);
        let acls: Vec<Acl> = (0..count).map(|_| random_acl(&mut rng, alphabet)).collect();
        let rules_open = Rules {
            default_allow: true,
            super_users: BTreeSet::new(),
        };
        let rules_closed = Rules {
            default_allow: false,
            super_users: BTreeSet::new(),
        };
        let set = AclSet::new(acls.iter().cloned());
        for _ in 0..30 {
            let (principal, host) = (*rng.pick(&CALLERS), *rng.pick(&CALLER_HOSTS));
            let kind = resource::TOPIC + rng.below(6) as i8;
            let name = rng.name(alphabet, 4, 0);
            let op = operation::ALL + rng.below(14) as i8;
            let who = Subject { principal, host };
            let res = Resource { kind, name: &name };
            let open = rules_open.authorize(&set, who, res, op);
            let closed = rules_closed.authorize(&set, who, res, op);
            match (open, closed) {
                (true, true) => allowed += 1,
                (false, false) => denied += 1,
                (true, false) => default_used += 1,
                (false, true) => {
                    unreachable!("an open default cannot refuse what a closed one allows")
                }
            }
            let covered = acls
                .iter()
                .any(|a| a.resource_type == kind && a.covers(&name));
            if covered && !open {
                covered_but_unmatched += 1;
            }
            let denying = acls.iter().any(|a| {
                a.resource_type == kind
                    && a.covers(&name)
                    && a.permission == permission::DENY
                    && (a.operation == op || a.operation == operation::ALL)
                    && (a.principal == principal || a.principal == "User:*")
                    && (a.host == "*" || a.host == host)
            });
            let allowing = acls.iter().any(|a| {
                a.resource_type == kind
                    && a.covers(&name)
                    && a.permission == permission::ALLOW
                    && (a.principal == principal || a.principal == "User:*")
                    && (a.host == "*" || a.host == host)
            });
            if denying && allowing {
                deny_over_allow += 1;
            }
            let explicit = acls.iter().any(|a| {
                a.resource_type == kind
                    && a.covers(&name)
                    && a.permission == permission::ALLOW
                    && (a.operation == op || a.operation == operation::ALL)
                    && (a.principal == principal || a.principal == "User:*")
                    && (a.host == "*" || a.host == host)
            });
            if closed && !explicit {
                implied += 1;
            }
        }
    }
    for (what, n) in [
        ("allowed", allowed),
        ("denied", denied),
        (
            "the default allowed what a covered resource would refuse",
            default_used,
        ),
        (
            "covered by an ACL that is not about the caller",
            covered_but_unmatched,
        ),
        ("a deny over an allow", deny_over_allow),
        (
            "an operation implied by an ACL of another operation",
            implied,
        ),
    ] {
        assert!(n >= 10, "the random sets rarely produce {what}: {n}");
    }
}
