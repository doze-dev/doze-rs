//! The Access page: DescribeAcls and DescribeUserScramCredentials through the protocol (names and mechanism numbers are all that
//! is read of a user), and `ConsoleView` for what the protocol hides (who the super users are, which users came from flags).
//! No secret is read here, so none can be shown.

use kafka_wire::api::Response;

use crate::access_words::{Switches, warnings};
use crate::backend::{Backend, requests::acls};
use crate::facts::Facts;
use crate::facts_start::StartFacts;
use crate::model::{
    AccessAcls, AccessAuth, AccessAuthz, AccessConsole, AccessFrom, AccessTls, AccessUser,
    AccessView, AclEntry, AclPattern, AclPermission, Refusal,
};
use crate::refusal::refusal;

/// The most ACLs the table lists.
pub const MOST_ACLS: usize = 500;
/// The most users the table lists.
pub const MOST_USERS: usize = 500;

/// The console's principal as Kafka writes it.
pub const CONSOLE_PRINCIPAL: &str = "User:__console";

/// The ACLs as DescribeAcls lists them, in its order, or the broker's refusal.
///
/// # Errors
/// The broker's own words if it refused to describe them (or said nothing).
pub async fn entries(b: &Backend) -> Result<Vec<AclEntry>, Refusal> {
    let Ok(Response::DescribeAcls(r)) = b.call(acls::describe_all()).await else {
        return Err(refusal(-1, Some("The broker did not answer.")));
    };
    if r.error_code != 0 {
        return Err(refusal(
            r.error_code,
            r.error_message.as_ref().map(|m| m.as_str()),
        ));
    }
    let mut out = Vec::new();
    for res in &r.resources {
        let pattern = match res.pattern_type {
            3 => AclPattern::Literal,
            4 => AclPattern::Prefixed,
            _ => continue,
        };
        for a in &res.acls {
            let permission = match a.permission_type {
                3 => AclPermission::Allow,
                2 => AclPermission::Deny,
                _ => continue,
            };
            out.push(AclEntry {
                kind: res.resource_type,
                name: res.resource_name.to_string(),
                pattern,
                principal: a.principal.to_string(),
                host: a.host.to_string(),
                operation: a.operation,
                permission,
            });
        }
    }
    Ok(out)
}

/// A SCRAM credential as listed: the mechanism and its iteration count.
type Scram = (&'static str, u32);

fn mechanism(n: i8) -> Option<&'static str> {
    match n {
        1 => Some("SCRAM-SHA-256"),
        2 => Some("SCRAM-SHA-512"),
        _ => None,
    }
}

/// The users that have a SCRAM credential, by name, with the mechanisms. Only the names and the mechanism numbers are read:
/// the iteration count is read too (a public parameter every client is sent), and no other part of a credential is in the answer.
async fn scram_users(b: &Backend) -> Vec<(String, Vec<Scram>)> {
    let Ok(Response::DescribeUserScramCredentials(r)) = b.call(acls::describe_users()).await else {
        return Vec::new();
    };
    let mut out: Vec<(String, Vec<Scram>)> = r
        .results
        .iter()
        .filter(|u| u.error_code == 0)
        .map(|u| {
            (
                u.user.to_string(),
                u.credential_infos
                    .iter()
                    .filter_map(|c| {
                        Some((mechanism(c.mechanism)?, u32::try_from(c.iterations).ok()?))
                    })
                    .collect(),
            )
        })
        .collect();
    out.sort();
    out.truncate(MOST_USERS);
    out
}

/// The users: the SCRAM ones the protocol lists, and the flag users that only PLAIN knows.
#[must_use]
pub fn users_of(
    scram: Vec<(String, Vec<Scram>)>,
    flag_users: &[String],
    plain_on: bool,
    super_users: &[String],
) -> Vec<AccessUser> {
    let mut rows: Vec<AccessUser> = scram
        .into_iter()
        .map(|(name, m)| AccessUser {
            from: if flag_users.contains(&name) {
                AccessFrom::Flags
            } else {
                AccessFrom::Stored
            },
            super_user: super_users.contains(&format!("User:{name}")),
            mechanisms: m.iter().map(|(n, _)| (*n).to_owned()).collect(),
            iterations: m.iter().map(|(n, i)| ((*n).to_owned(), *i)).collect(),
            name,
        })
        .collect();
    for name in flag_users {
        match rows.iter_mut().find(|r| &r.name == name) {
            Some(r) if plain_on => r.mechanisms.push("PLAIN".to_owned()),
            Some(_) => {}
            None => rows.push(AccessUser {
                name: name.clone(),
                from: AccessFrom::Flags,
                iterations: vec![],
                mechanisms: if plain_on {
                    vec!["PLAIN".to_owned()]
                } else {
                    vec![]
                },
                super_user: super_users.contains(&format!("User:{name}")),
            }),
        }
    }
    rows.sort_by(|a, b| (a.from as u8, &a.name).cmp(&(b.from as u8, &b.name)));
    rows
}

/// What the broker said, before it is made into a page.
#[derive(Debug, Clone)]
pub struct Read {
    pub flag_users: Vec<String>,
    pub supers: Vec<String>,
    pub default_allow: bool,
    pub stored: usize,
    pub acls: Result<Vec<AclEntry>, Refusal>,
    pub scram: Vec<(String, Vec<Scram>)>,
}

/// The page.
pub async fn access(b: &Backend, facts: &Facts, start: &StartFacts) -> AccessView {
    let known = b.access_facts();
    let (flag_users, supers, default_allow, stored) = known.as_ref().map_or_else(
        || {
            (
                vec![],
                vec![],
                b.instance().is_none_or(|i| i.acl_default_allow),
                0,
            )
        },
        |k| {
            (
                k.flag_users.clone(),
                k.super_users.clone(),
                k.acl_default_allow,
                k.stored_users,
            )
        },
    );
    let read = Read {
        flag_users,
        supers,
        default_allow,
        stored,
        acls: entries(b).await,
        scram: scram_users(b).await,
    };
    assemble(read, facts, start)
}

/// The page from what was read (pure: everything it says is in the arguments).
#[must_use]
pub fn assemble(read: Read, facts: &Facts, start: &StartFacts) -> AccessView {
    let Read {
        flag_users,
        supers,
        default_allow,
        stored,
        acls,
        scram,
    } = read;
    let (rows, refused) = match acls {
        Ok(rows) => (rows, None),
        Err(r) => (vec![], Some(r)),
    };
    let allow = rows
        .iter()
        .filter(|e| e.permission == AclPermission::Allow)
        .count();
    let sasl = !facts.sasl.is_empty();
    let plain_on = facts.sasl.iter().any(|m| m == "PLAIN");
    let console_addr = start
        .console_addr
        .clone()
        .unwrap_or_else(|| "unknown".to_owned());
    let listen = facts.bootstrap();
    let switches = Switches {
        sasl,
        tls: facts.tls,
        default_allow,
        acls_stored: !rows.is_empty(),
        listener_beyond_loopback: !facts.loopback(),
        console_beyond_loopback: facts.unprotected,
        console_unprotected: start.console_unprotected,
        console_addr: &console_addr,
    };
    let users = users_of(scram, &flag_users, plain_on, &supers);
    AccessView {
        auth: AccessAuth {
            on: sasl,
            mechanisms: facts.sasl.clone(),
            flag_users: flag_users.len(),
            stored_users: stored,
            failed_login: start
                .failed_login_delay
                .map(|d| format!("waits {}", crate::settings_words::spoken(d))),
            plain: plain_on,
        },
        authz: AccessAuthz {
            default_allow,
            acls: rows.len(),
            allow,
            deny: rows.len() - allow,
            super_users: supers,
        },
        tls: tls_of(facts, start),
        users,
        acls: AccessAcls {
            more: rows.len().saturating_sub(MOST_ACLS),
            rows: rows.into_iter().take(MOST_ACLS).collect(),
            refused,
        },
        console: console_of(facts, &console_addr),
        warnings: warnings(&switches),
        listen,
    }
}

fn tls_of(facts: &Facts, start: &StartFacts) -> AccessTls {
    AccessTls {
        on: facts.tls,
        cert: start.tls_cert.clone(),
        listener: facts.bootstrap(),
        loopback: facts.loopback(),
    }
}

fn console_of(facts: &Facts, addr: &str) -> AccessConsole {
    AccessConsole {
        principal: CONSOLE_PRINCIPAL.to_owned(),
        listens: addr.to_owned(),
        beyond_loopback: facts.unprotected,
        allow_hosts: facts.allow_hosts.clone(),
    }
}

#[cfg(test)]
mod tests;
