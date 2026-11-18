//! Windows: a route is a Name Resolution Policy Table rule sending a namespace to the resolver (the zone's own rule is
//! setup's; a domain's carries doze's route comment), and the authority is a root in the machine's Root store. Both are
//! read through PowerShell, which every Windows since 8 ships with these cmdlets and the `Cert:` drive; reading needs
//! no administrator.

use super::{CA_COMMON_NAME, output, valid_fingerprint};
use crate::ca::Ca;

/// The comment on a domain route's NRPT rule (the zone's own rule carries setup's).
pub(crate) const ROUTE_COMMENT: &str = "doze route";

/// One NRPT rule: its namespaces, the servers it sends them to, and its comment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub namespaces: Vec<String>,
    pub servers: Vec<String>,
    pub comment: String,
}

/// Lists the rules, one per line: namespaces, servers and comment, `|`-separated, lists `,`-separated.
pub(crate) const LIST_RULES: &str = "Get-DnsClientNrptRule | ForEach-Object { \"$($_.Namespace -join ',')|$($_.NameServers -join ',')|$($_.Comment)\" }";

/// The rules in the output of `LIST_RULES` (`Get-DnsClientNrptRule`, one per line).
// fuzz:parser fuzz_setup
pub fn fields(out: &str) -> Vec<Rule> {
    let list = |s: &str| -> Vec<String> {
        s.split(',')
            .map(str::trim)
            .filter(|x| !x.is_empty())
            .map(str::to_string)
            .collect()
    };
    out.lines()
        .filter_map(|line| {
            let mut parts = line.trim_end_matches('\r').splitn(3, '|');
            let (ns, servers) = (parts.next()?, parts.next()?);
            Some(Rule {
                namespaces: list(ns),
                servers: list(servers),
                comment: parts.next().unwrap_or_default().trim().to_string(),
            })
        })
        .collect()
}

/// The machine's NRPT rules (none when PowerShell or the cmdlets are not there).
pub(crate) fn rules() -> Vec<Rule> {
    powershell(LIST_RULES)
        .map(|o| fields(&o))
        .unwrap_or_default()
}

/// Runs a PowerShell command, quietly, returning its output when it succeeded.
pub(crate) fn powershell(command: &str) -> Option<String> {
    output(
        "powershell",
        &["-NoProfile", "-NonInteractive", "-Command", command],
    )
}

fn route_rule(domain: &str) -> Option<Rule> {
    let suffix = format!(".{domain}");
    rules().into_iter().find(|r| r.namespaces.contains(&suffix))
}

fn ours(r: &Rule) -> bool {
    let ip = crate::resolver_addr();
    let ip = ip.rsplit_once(':').map_or(ip.as_str(), |(h, _)| h);
    r.comment == ROUTE_COMMENT && r.servers.iter().any(|s| s == ip)
}

pub(super) fn domain_routed(domain: &str) -> (bool, String) {
    match route_rule(domain) {
        Some(r) if ours(&r) => (
            true,
            format!("an NRPT rule sends {domain} to {}", r.servers.join(", ")),
        ),
        Some(r) => (
            false,
            format!(
                "an NRPT rule that is not doze's sends {domain} to {}; doze leaves it alone — remove it to let doze \
                 route {domain}",
                r.servers.join(", ")
            ),
        ),
        None => (
            false,
            format!("{domain} is not routed to the doze resolver"),
        ),
    }
}

pub(super) fn route_foreign(domain: &str) -> bool {
    route_rule(domain).is_some_and(|r| !ours(&r))
}

/// SHA-256 of each certificate the machine's Root store holds (with `filter`, only those it keeps), one per line.
fn root_fingerprints(filter: &str) -> Vec<String> {
    let cmd = format!(
        "$h = [Security.Cryptography.SHA256]::Create(); Get-ChildItem Cert:\\LocalMachine\\Root {filter}| \
         ForEach-Object {{ ([BitConverter]::ToString($h.ComputeHash($_.RawData)) -replace '-','').ToLower() }}"
    );
    powershell(&cmd)
        .map(|o| {
            o.lines()
                .map(str::trim)
                .filter(|fp| valid_fingerprint(fp))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn ca_trusted(ca: &Ca) -> (bool, String) {
    if root_fingerprints("").contains(&ca.fingerprint()) {
        return (
            true,
            format!(
                "trusted in the machine's Root store · {} · expires {}",
                ca.common_name(),
                ca.expires()
            ),
        );
    }
    (false, format!("not trusted · {}", ca.root_path().display()))
}

pub(super) fn fingerprint_trusted(fp: &str) -> bool {
    root_fingerprints("")
        .iter()
        .any(|have| have.eq_ignore_ascii_case(fp))
}

pub(super) fn doze_roots() -> Vec<String> {
    root_fingerprints(&format!(
        "| Where-Object {{ $_.Subject -like 'CN={CA_COMMON_NAME}*' }} "
    ))
}
