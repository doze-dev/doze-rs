//! The authorizer against a second, independent transcription of Kafka's own (`model.rs`), on random ACL sets and requests
//! from a seeded generator, and the directed case that the first model missed (a mutation of `wildcard_allow` in
//! `by_type.rs` survived it).
//!
//! The names are drawn from an alphabet that reaches the neighbourhood of `hardcode`, the name `authorizeByResourceType`
//! asks first (`h`, `hard`, `code`, `hardcode`), beside the characters that sort around `*`, the astral and the private-use
//! ones. Where Kafka itself throws or loops (SEC-D81) the model says so and the case is not compared.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::too_many_lines,
    clippy::too_many_arguments,
    clippy::type_complexity,
    unreachable_pub
)]

mod model;

use std::cmp::Ordering;
use std::collections::{BTreeSet, HashSet};

use kafka_auth::authz::{Acl, AclSet, Resource, Rules, Subject};
use model::{M, cmp, model_authorize, model_by_type, u};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.below(xs.len())]
    }
}

const ALPHA: &[&str] = &[
    "a",
    "b",
    "*",
    "!",
    "~",
    "h",
    "hard",
    "code",
    "hardcode",
    "\u{e9}",
    "\u{1F600}",
    "\u{ff5e}",
    "\u{e000}",
];

fn name(r: &mut Rng, allow_empty: bool) -> String {
    loop {
        let n = r.below(5);
        let s: String = (0..n).map(|_| *r.pick(ALPHA)).collect();
        if !s.is_empty() || allow_empty {
            return s;
        }
    }
}

const PRINCIPALS: &[&str] = &[
    "User:a", "User:b", "User:*", "Group:x", "User:a:b", "User:", "User:su", "User:*x", "user:a",
];
const HOSTS: &[&str] = &["*", "1.2.3.4", "0:0:0:0:0:0:0:1", "::1", "5.6.7.8"];

fn run(seed: u64, cases: usize, max_acls: usize, types: i8) -> (usize, usize, Vec<String>, usize) {
    let mut r = Rng(seed);
    let mut dis = Vec::new();
    let mut throws = 0usize;
    let mut checked = 0usize;
    for _ in 0..cases {
        let n = r.below(max_acls + 1);
        let mut real = Vec::new();
        for _ in 0..n {
            real.push(Acl {
                resource_type: 2 + r.below(types as usize) as i8,
                name: name(&mut r, false),
                pattern_type: 3 + r.below(2) as i8,
                principal: (*r.pick(PRINCIPALS)).to_owned(),
                host: (*r.pick(HOSTS)).to_owned(),
                operation: 2 + r.below(14) as i8,
                permission: 2 + r.below(2) as i8,
            });
        }
        let mut model: Vec<M> = real
            .iter()
            .map(|a| M {
                rt: a.resource_type,
                name: u(&a.name),
                pt: a.pattern_type,
                principal: u(&a.principal),
                host: u(&a.host),
                op: a.operation,
                perm: a.permission,
            })
            .collect();
        model.sort_by(cmp);
        model.dedup_by(|a, b| cmp(a, b) == Ordering::Equal);
        let set = AclSet::new(real.clone());
        let default_allow = r.below(2) == 0;
        let mut supers = HashSet::new();
        supers.insert("User:su".to_owned());
        let rules = Rules {
            default_allow,
            super_users: supers.iter().cloned().collect::<BTreeSet<_>>(),
        };
        for _ in 0..12 {
            let principal = *r.pick(&[
                "User:a",
                "User:b",
                "User:su",
                "Group:x",
                "User:a:b",
                "User:ANONYMOUS",
                "User:*",
            ]);
            let host = *r.pick(&["1.2.3.4", "0:0:0:0:0:0:0:1", "9.9.9.9", "", "5.6.7.8"]);
            let rt = 2 + r.below(types as usize) as i8;
            let rn = if r.below(3) == 0 && !real.is_empty() {
                let a = r.pick(&real);
                if r.below(2) == 0 {
                    a.name.clone()
                } else {
                    format!("{}{}", a.name, name(&mut r, true))
                }
            } else {
                name(&mut r, true)
            };
            let op = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 99, 0][r.below(18)];
            let want =
                model_authorize(&model, default_allow, &supers, principal, host, rt, &rn, op);
            let got = rules.authorize(
                &set,
                Subject { principal, host },
                Resource {
                    kind: rt,
                    name: &rn,
                },
                op,
            );
            checked += 1;
            match want {
                Err(e) => {
                    throws += 1;
                    if dis.len() < 15 {
                        dis.push(format!(
                            "KAFKA-THROWS({e}) got={got} p={principal} h={host:?} rt={rt} name={rn:?} op={op} default={default_allow} acls={real:?}"
                        ));
                    }
                }
                Ok(w) if w != got => {
                    if dis.len() < 15 {
                        dis.push(format!(
                            "DISAGREE kafka={w} author={got} p={principal} h={host:?} rt={rt} name={rn:?} op={op} default={default_allow} acls={model_dbg:?}",
                            model_dbg = real
                        ));
                    }
                    dis.push(String::from("D"));
                }
                _ => {}
            }
            // by-type
            if (2..=15).contains(&op) {
                let w = model_by_type(&model, default_allow, &supers, principal, host, op, rt);
                let g = rules.authorize_by_resource_type(&set, Subject { principal, host }, op, rt);
                checked += 1;
                match w {
                    Ok(w) if w != g => {
                        dis.push(format!(
                            "BYTYPE-DISAGREE kafka={w} author={g} p={principal} h={host:?} rt={rt} op={op} default={default_allow} acls={real:?}"
                        ));
                    }
                    Err(e) => dis.push(format!("BYTYPE-THROWS {e}")),
                    _ => {}
                }
            }
        }
    }
    let real_dis = dis
        .iter()
        .filter(|d| !d.starts_with("KAFKA-THROWS") && *d != "D")
        .count();
    let dcount = dis.iter().filter(|d| *d == "D").count();
    (checked, throws, dis, real_dis + dcount)
}

/// How many times fewer random ACL sets each of the four runs draws than the audit did: 100,000 sets of 12 requests, 2.13
/// million checks, which took about a minute in a debug build. A third is about 710,000 checks and a few seconds.
const SCALE_DOWN: usize = 3;

#[test]
fn the_authorizer_agrees_with_an_independent_transcription_of_kafka() {
    let mut total = 0;
    let mut throws = 0;
    let mut bad = 0;
    let mut shown = Vec::new();
    for (seed, cases, max_acls, types) in [
        (0x9e37_79b9_7f4a_7c15_u64, 30000, 6, 1_i8),
        (0x1234_5678_9abc_def1, 30000, 12, 2),
        (0xdead_beef_cafe_f00d, 20000, 40, 4),
        (0x0bad_c0de_1234_5677, 20000, 3, 1),
    ] {
        let (c, t, d, b) = run(seed, cases / SCALE_DOWN, max_acls, types);
        total += c;
        throws += t;
        bad += b;
        shown.extend(
            d.into_iter()
                .filter(|x| x != "D" && !x.starts_with("KAFKA-THROWS")),
        );
    }
    eprintln!("checked {total}, Kafka throws or loops in {throws}, disagreements {bad}");
    for d in shown.iter().take(20) {
        eprintln!("{d}");
    }
    assert!(
        throws > 0,
        "the generator no longer reaches the names on which Kafka's own walk fails"
    );
    assert_eq!(bad, 0, "disagreements");
}

/// Directed: Kafka returns ALLOWED from authorizeByResourceType for an ALLOW of `*` even when a DENY covers the name
/// `hardcode` that is asked first (a DENY of a prefix is only looked at for the other allows).
#[test]
fn a_wildcard_allow_survives_a_deny_of_the_hardcoded_name() {
    let acl = |name: &str, pattern: i8, permission: i8| Acl {
        resource_type: 2,
        name: name.to_owned(),
        pattern_type: pattern,
        principal: "User:u".to_owned(),
        host: "*".to_owned(),
        operation: 4,
        permission,
    };
    let rules = Rules::default();
    let who = Subject {
        principal: "User:u",
        host: "1.2.3.4",
    };
    for deny in [acl("hardcode", 3, 2), acl("h", 4, 2), acl("hardcode", 4, 2)] {
        let set = AclSet::new([acl("*", 3, 3), deny.clone()]);
        assert!(
            !rules.authorize(
                &set,
                who,
                Resource {
                    kind: 2,
                    name: "hardcode"
                },
                4
            ),
            "{deny:?}"
        );
        assert!(
            rules.authorize_by_resource_type(&set, who, 4, 2),
            "{deny:?}"
        );
    }
}
