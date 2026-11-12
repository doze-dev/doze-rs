//! A second, independent transcription of Kafka 4.3.1's `StandardAuthorizerData.authorize`, `findAclRule`, `checkSection`,
//! `findResult` and `MatchingRuleBuilder`, and of `Authorizer.authorizeByResourceType`: Java UTF-16 strings, the ACLs in a
//! sorted set (resource type, name in REVERSE, pattern type, operation, principal, host, permission) walked from an
//! exemplar. Written apart from `authz_model/model.rs` (a different reading of the same source, by a different hand).
//! A case in which Kafka itself throws (see SEC-D81) is returned as `Err` and is not compared.

use std::cmp::Ordering;
use std::collections::HashSet;

pub type S = Vec<u16>;
pub fn u(s: &str) -> S {
    s.encode_utf16().collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct M {
    pub rt: i8,
    pub name: S,
    pub pt: i8,
    pub principal: S,
    pub host: S,
    pub op: i8,
    pub perm: i8,
}

pub fn cmp(a: &M, b: &M) -> Ordering {
    a.rt.cmp(&b.rt)
        .then_with(|| b.name.cmp(&a.name)) // reverse
        .then_with(|| a.pt.cmp(&b.pt))
        .then_with(|| a.op.cmp(&b.op))
        .then_with(|| a.principal.cmp(&b.principal))
        .then_with(|| a.host.cmp(&b.host))
        .then_with(|| a.perm.cmp(&b.perm))
}

pub const LITERAL: i8 = 3;
pub const ALLOW: i8 = 3;
pub const DENY: i8 = 2;
pub const ALL: i8 = 2;
pub const DESCRIBE: i8 = 8;
pub const DESCRIBE_CONFIGS: i8 = 10;

fn parse_principal(p: &S) -> Option<(S, S)> {
    let i = p.iter().position(|c| *c == u16::from(b':'))?;
    Some((p[..i].to_vec(), p[i + 1..].to_vec()))
}

struct Builder {
    deny: bool,
    allow: bool,
    has: bool,
}

fn matches_up_to(r: &S, p: &S) -> usize {
    let mut i = 0;
    loop {
        if r.len() == i || p.len() == i || r[i] != p[i] {
            break;
        }
        i += 1;
    }
    i
}

fn lower_bound(acls: &[M], ex: &M) -> usize {
    acls.partition_point(|a| cmp(a, ex) == Ordering::Less)
}

fn find_result(
    op: i8,
    principals: &[(S, S)],
    host: &S,
    acl: &M,
) -> Option<bool> {
    let ap = parse_principal(&acl.principal).expect("stored acls have a colon");
    if !principals.contains(&ap) {
        return None;
    }
    if acl.host != u("*") && &acl.host != host {
        return None;
    }
    if acl.op != ALL {
        if acl.perm == ALLOW {
            match op {
                DESCRIBE => {
                    if ![8, 3, 4, 6, 7].contains(&acl.op) {
                        return None;
                    }
                }
                DESCRIBE_CONFIGS => {
                    if ![10, 11].contains(&acl.op) {
                        return None;
                    }
                }
                _ => {
                    if op != acl.op {
                        return None;
                    }
                }
            }
        } else if op != acl.op {
            return None;
        }
    }
    Some(acl.perm == ALLOW)
}

pub fn model_authorize(
    acls: &[M],
    default_allow: bool,
    supers: &HashSet<String>,
    principal: &str,
    host: &str,
    rt: i8,
    name: &str,
    op: i8,
) -> Result<bool, String> {
    if supers.contains(principal) {
        return Ok(true);
    }
    let pp = parse_principal(&u(principal)).unwrap();
    let principals = vec![pp, (u("User"), u("*"))];
    let host = u(host);
    let rname = u(name);
    let mut b = Builder {
        deny: false,
        allow: false,
        has: false,
    };
    // first section
    let ex = M {
        rt,
        name: rname.clone(),
        pt: 0,
        principal: vec![],
        host: vec![],
        op: 0,
        perm: 0,
    };
    check_section_real(acls, rt, &rname, ex, &principals, &host, op, &mut b)?;
    if b.deny {
        return Ok(false);
    }
    let ex = M {
        rt,
        name: u("*"),
        pt: LITERAL,
        principal: vec![],
        host: vec![],
        op: 0,
        perm: 0,
    };
    check_section_real(acls, rt, &rname, ex, &principals, &host, op, &mut b)?;
    Ok(if b.deny {
        false
    } else if b.allow {
        true
    } else if !b.has {
        default_allow
    } else {
        false
    })
}

#[allow(clippy::too_many_arguments)]
fn check_section_real(
    acls: &[M],
    rt: i8,
    rname: &S,
    mut ex: M,
    principals: &[(S, S)],
    host: &S,
    op: i8,
    b: &mut Builder,
) -> Result<(), String> {
    let mut it = lower_bound(acls, &ex);
    let mut steps = 0usize;
    while it < acls.len() {
        steps += 1;
        if steps > 5_000 {
            return Err("loop".into());
        }
        let acl = &acls[it];
        it += 1;
        if acl.rt != rt {
            break;
        }
        let m = matches_up_to(rname, &acl.name);
        if m == acl.name.len() {
            if acl.pt == LITERAL && m != rname.len() {
                continue;
            }
        } else if !(acl.name == u("*") && acl.pt == LITERAL) {
            if m > ex.name.len() {
                return Err("StringIndexOutOfBounds".into());
            }
            ex = M {
                name: ex.name[..m].to_vec(),
                ..ex
            };
            it = lower_bound(acls, &ex);
            continue;
        }
        b.has = true;
        match find_result(op, principals, host, acl) {
            Some(true) => b.allow = true,
            Some(false) => {
                b.deny = true;
                return Ok(());
            }
            None => {}
        }
    }
    Ok(())
}

// authorizeByResourceType, transcribed.
pub fn model_by_type(
    acls: &[M],
    default_allow: bool,
    supers: &HashSet<String>,
    principal: &str,
    host: &str,
    op: i8,
    rt: i8,
) -> Result<bool, String> {
    if model_authorize(acls, default_allow, supers, principal, host, rt, "hardcode", op)? {
        return Ok(true);
    }
    let me = parse_principal(&u(principal)).unwrap();
    let mut deny_lit: HashSet<S> = HashSet::new();
    let mut deny_pre: HashSet<S> = HashSet::new();
    let mut allow_lit: HashSet<S> = HashSet::new();
    let mut allow_pre: HashSet<S> = HashSet::new();
    let mut wild_allow = false;
    let hostv = u(host);
    for b in acls.iter().filter(|a| a.rt == rt) {
        if b.host != hostv && b.host != u("*") {
            continue;
        }
        if parse_principal(&b.principal).unwrap() != me && b.principal != u("User:*") {
            continue;
        }
        if b.op != op && b.op != ALL {
            continue;
        }
        if b.perm == DENY {
            match b.pt {
                3 => {
                    if b.name == u("*") {
                        return Ok(false);
                    }
                    deny_lit.insert(b.name.clone());
                }
                4 => {
                    deny_pre.insert(b.name.clone());
                }
                _ => {}
            }
            continue;
        }
        if b.perm != ALLOW {
            continue;
        }
        match b.pt {
            3 => {
                if b.name == u("*") {
                    wild_allow = true;
                    continue;
                }
                allow_lit.insert(b.name.clone());
            }
            4 => {
                allow_pre.insert(b.name.clone());
            }
            _ => {}
        }
    }
    if wild_allow {
        return Ok(true);
    }
    for (lit, set) in [(true, &allow_lit), (false, &allow_pre)] {
        for s in set {
            if lit && deny_lit.contains(s) {
                continue;
            }
            let mut dominated = false;
            for i in 1..=s.len() {
                if deny_pre.contains(&s[..i]) {
                    dominated = true;
                    break;
                }
            }
            if !dominated {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

