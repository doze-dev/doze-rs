//! `StandardAuthorizerData.authorize` and `Authorizer.authorizeByResourceType` of Kafka 4.3.1, transcribed.

use std::cmp::Ordering;
use std::collections::{BTreeSet, HashSet};

use kafka_auth::authz::{Acl, operation, pattern, permission};

pub(crate) struct Model {
    acls: Vec<Acl>,
    supers: BTreeSet<String>,
    default_allow: bool,
}

#[derive(Default)]
struct Builder {
    deny: bool,
    allow: bool,
    has_resource_acls: bool,
}

/// `StandardAcl.compareTo`.
fn java_order(a: &Acl, b: &Acl) -> Ordering {
    a.resource_type
        .cmp(&b.resource_type)
        .then_with(|| b.name.cmp(&a.name))
        .then_with(|| a.pattern_type.cmp(&b.pattern_type))
        .then_with(|| a.operation.cmp(&b.operation))
        .then_with(|| a.principal.cmp(&b.principal))
        .then_with(|| a.host.cmp(&b.host))
        .then_with(|| a.permission.cmp(&b.permission))
}

fn exemplar(kind: i8, name: &str, pattern_type: i8) -> Acl {
    Acl {
        resource_type: kind,
        name: name.to_owned(),
        pattern_type,
        principal: String::new(),
        host: String::new(),
        operation: 0,
        permission: 0,
    }
}

fn matches_up_to(resource: &str, pattern: &str) -> usize {
    resource
        .chars()
        .zip(pattern.chars())
        .take_while(|(a, b)| a == b)
        .count()
}

/// `findResult`: `Some(true)` is ALLOWED, `Some(false)` DENIED, `None` is null.
fn find_result(op: i8, principals: &[&str; 2], host: &str, acl: &Acl) -> Option<bool> {
    if !principals.contains(&acl.principal.as_str()) {
        return None;
    }
    if acl.host != "*" && acl.host != host {
        return None;
    }
    let implies_describe = [
        operation::DESCRIBE,
        operation::READ,
        operation::WRITE,
        operation::DELETE,
        operation::ALTER,
    ];
    let implies_describe_configs = [operation::DESCRIBE_CONFIGS, operation::ALTER_CONFIGS];
    if acl.operation != operation::ALL {
        if acl.permission == permission::ALLOW {
            match op {
                operation::DESCRIBE => {
                    if !implies_describe.contains(&acl.operation) {
                        return None;
                    }
                }
                operation::DESCRIBE_CONFIGS => {
                    if !implies_describe_configs.contains(&acl.operation) {
                        return None;
                    }
                }
                _ => {
                    if op != acl.operation {
                        return None;
                    }
                }
            }
        } else if op != acl.operation {
            return None;
        }
    }
    Some(acl.permission == permission::ALLOW)
}

impl Model {
    pub(crate) fn new(acls: &[Acl], supers: &[&str], default_allow: bool) -> Self {
        let mut acls = acls.to_vec();
        acls.sort_by(java_order);
        acls.dedup();
        Self {
            acls,
            supers: supers.iter().map(|s| (*s).to_owned()).collect(),
            default_allow,
        }
    }

    /// `tailSet(exemplar, true)`: where its iteration starts.
    fn tail(&self, from: &Acl) -> usize {
        self.acls
            .partition_point(|a| java_order(a, from) == Ordering::Less)
    }

    fn check_section(
        &self,
        action: (i8, &str, i8),
        (principals, host): (&[&str; 2], &str),
        mut ex: Acl,
        b: &mut Builder,
    ) {
        let (kind, resource_name, op) = action;
        let mut i = self.tail(&ex);
        let mut steps = 0;
        while let Some(acl) = self.acls.get(i) {
            steps += 1;
            assert!(steps < 100_000, "the model does not terminate");
            i += 1;
            if acl.resource_type != kind {
                break;
            }
            let up = matches_up_to(resource_name, &acl.name);
            if up == acl.name.chars().count() {
                if acl.pattern_type == pattern::LITERAL && up != resource_name.chars().count() {
                    continue;
                }
            } else if !(acl.name == "*" && acl.pattern_type == pattern::LITERAL) {
                ex.name = ex.name.chars().take(up).collect();
                i = self.tail(&ex);
                continue;
            }
            b.has_resource_acls = true;
            match find_result(op, principals, host, acl) {
                Some(true) => b.allow = true,
                Some(false) => {
                    b.deny = true;
                    return;
                }
                None => {}
            }
        }
    }

    pub(crate) fn authorize(
        &self,
        principal: &str,
        host: &str,
        kind: i8,
        name: &str,
        op: i8,
    ) -> bool {
        if self.supers.contains(principal) {
            return true;
        }
        let principals = [principal, "User:*"];
        let mut b = Builder::default();
        let ears = (&principals, host);
        self.check_section((kind, name, op), ears, exemplar(kind, name, 0), &mut b);
        if !b.deny {
            let star = exemplar(kind, "*", pattern::LITERAL);
            self.check_section((kind, name, op), ears, star, &mut b);
        }
        if b.deny {
            false
        } else if b.allow {
            true
        } else if !b.has_resource_acls {
            self.default_allow
        } else {
            false
        }
    }

    /// `Authorizer.authorizeByResourceType`.
    pub(crate) fn authorize_by_type(&self, principal: &str, host: &str, op: i8, kind: i8) -> bool {
        if self.authorize(principal, host, kind, "hardcode", op) {
            return true;
        }
        let mut deny_literal: HashSet<&str> = HashSet::new();
        let mut deny_prefixed: HashSet<&str> = HashSet::new();
        let mut allow_literal: HashSet<&str> = HashSet::new();
        let mut allow_prefixed: HashSet<&str> = HashSet::new();
        let mut has_wildcard_allow = false;
        for a in self.acls.iter().filter(|a| a.resource_type == kind) {
            if a.host != host && a.host != "*" {
                continue;
            }
            if a.principal != principal && a.principal != "User:*" {
                continue;
            }
            if a.operation != op && a.operation != operation::ALL {
                continue;
            }
            if a.permission == permission::DENY {
                match a.pattern_type {
                    pattern::LITERAL => {
                        if a.name == "*" {
                            return false;
                        }
                        deny_literal.insert(&a.name);
                    }
                    pattern::PREFIXED => {
                        deny_prefixed.insert(&a.name);
                    }
                    _ => {}
                }
                continue;
            }
            if a.permission != permission::ALLOW {
                continue;
            }
            match a.pattern_type {
                pattern::LITERAL => {
                    if a.name == "*" {
                        has_wildcard_allow = true;
                        continue;
                    }
                    allow_literal.insert(&a.name);
                }
                pattern::PREFIXED => {
                    allow_prefixed.insert(&a.name);
                }
                _ => {}
            }
        }
        if has_wildcard_allow {
            return true;
        }
        for (literal, set) in [(true, &allow_literal), (false, &allow_prefixed)] {
            for allow in set {
                if literal && deny_literal.contains(allow) {
                    continue;
                }
                let mut sb = String::new();
                let mut dominated = false;
                for ch in allow.chars() {
                    sb.push(ch);
                    if deny_prefixed.contains(sb.as_str()) {
                        dominated = true;
                        break;
                    }
                }
                if !dominated {
                    return true;
                }
            }
        }
        false
    }
}
