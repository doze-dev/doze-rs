//! Windows: one Name Resolution Policy Table rule sends `.doze` to the resolver. Go had no Windows setup at all (a stub
//! that refused); this is new, designed here and proven on the elevated Windows runners (PLAN.md, N5).
//!
//! The NRPT is the system's own split DNS: the DNS client sends every name under a namespace to the rule's servers and
//! nothing else changes. Its servers are addresses only (no port), so the resolver listens on port 53 at an address
//! of its own ([`crate::RESOLVER_IP_WINDOWS`]). 127.0.0.0/8 is all local on Windows, as on Linux, so names need no
//! daemon to have addresses; no hosts block either, since the rule routes the whole zone, apex names included. The
//! rule is persistent (it lives in the registry), so nothing has to re-apply it at boot.

use super::run::ps_quote;
use super::{Plan, Shell, Status, Step};
use crate::trust_store::NrptRule;
use crate::{Error, SUFFIX};

/// The comment on the zone's rule: how setup finds its own and leaves everyone else's alone.
pub(crate) const ZONE_COMMENT: &str = "doze zone";

/// What setup found: the machine's rules and where the resolver listens.
#[derive(Debug, Clone)]
pub(crate) struct Facts {
    pub(crate) rules: Vec<NrptRule>,
    pub(crate) resolver: String,
}

impl Facts {
    pub(crate) fn read() -> Facts {
        Facts {
            rules: crate::trust_store::machine_nrpt_rules(),
            resolver: crate::resolver_addr(),
        }
    }

    /// The resolver's address, when the NRPT can reach it: on port 53.
    fn server(&self) -> Result<&str, Error> {
        match self.resolver.rsplit_once(':') {
            Some((ip, "53")) => Ok(ip),
            _ => Err(Error::Setup(format!(
                "the resolver is at {}, and the Name Resolution Policy Table sends only to port 53",
                self.resolver
            ))),
        }
    }
}

fn zone() -> String {
    format!(".{SUFFIX}")
}

pub(crate) fn check(f: &Facts) -> Status {
    let ns = zone();
    let ours = f
        .rules
        .iter()
        .find(|r| r.comment == ZONE_COMMENT && r.namespaces.contains(&ns));
    let other = f
        .rules
        .iter()
        .find(|r| r.comment != ZONE_COMMENT && r.namespaces.contains(&ns));
    let step = match (f.server(), ours) {
        (Err(e), _) => Step::new("resolver route", false, e.to_string()),
        (Ok(ip), Some(r)) if r.servers.iter().any(|s| s == ip) => Step::new(
            "resolver route",
            true,
            format!("an NRPT rule sends {ns} to {ip}"),
        ),
        (Ok(_), Some(r)) => Step::new(
            "resolver route",
            false,
            format!(
                "the NRPT rule sends {ns} to {}, not to the resolver — run setup again",
                r.servers.join(", ")
            ),
        ),
        (Ok(_), None) => {
            let mut detail = format!("no NRPT rule for {ns} — .{SUFFIX} will not resolve");
            if let Some(r) = other {
                detail = format!(
                    "{detail} (a rule that is not doze's, \"{}\", sends it to {})",
                    r.comment,
                    r.servers.join(", ")
                );
            }
            Step::new("resolver route", false, detail)
        }
    };
    Status {
        platform: "windows".into(),
        steps: vec![step],
    }
}

/// Removes the zone's rule (by its comment: nobody else's).
fn remove_zone_rule() -> String {
    format!(
        "Get-DnsClientNrptRule | Where-Object {{ $_.Comment -eq {} }} | ForEach-Object {{ Remove-DnsClientNrptRule -Name $_.Name -Force }}\n",
        ps_quote(ZONE_COMMENT)
    )
}

/// The install script: the rule replaced whole, so re-running cannot leave two, and the DNS client's cache cleared so a
/// failed lookup from before is not remembered.
pub(crate) fn install_plan(f: &Facts) -> Result<Option<Plan>, Error> {
    let ip = f.server()?;
    if check(f).ok() {
        return Ok(None);
    }
    let script = format!(
        "$ErrorActionPreference = 'Stop'\n{}Add-DnsClientNrptRule -Namespace {} -NameServers {} -Comment {}\nClear-DnsClientCache\n",
        remove_zone_rule(),
        ps_quote(&zone()),
        ps_quote(ip),
        ps_quote(ZONE_COMMENT),
    );
    Ok(Some(Plan {
        what: format!(
            "route .{SUFFIX} to the doze resolver ({ip}) with a Name Resolution Policy Table rule"
        ),
        shell: Shell::PowerShell,
        script,
    }))
}

pub(crate) fn uninstall_plan(cleanup: &[String]) -> Plan {
    let mut script = format!("$ErrorActionPreference = 'Stop'\n{}", remove_zone_rule());
    for line in cleanup {
        script.push_str(line);
        script.push('\n');
    }
    script.push_str("Clear-DnsClientCache\n");
    Plan {
        what: "remove the resolver route, the domains' routes and the certificate authority".into(),
        shell: Shell::PowerShell,
        script,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(ns: &str, server: &str, comment: &str) -> NrptRule {
        NrptRule {
            namespaces: vec![ns.into()],
            servers: vec![server.into()],
            comment: comment.into(),
        }
    }

    /// The rule is ours by its comment, points at the resolver, and is replaced whole; a resolver off port 53 is said,
    /// not routed somewhere it is not; someone else's rule is named and left alone.
    #[test]
    fn the_nrpt_rule_is_ours_replaced_whole_and_on_port_53() {
        let mut f = Facts {
            rules: vec![rule(".doze", "10.0.0.1", "corp")],
            resolver: "127.0.0.53:53".into(),
        };
        let st = check(&f);
        assert!(!st.ok() && st.steps[0].detail.contains("\"corp\""), "{st}");
        let plan = install_plan(&f).unwrap().unwrap();
        assert_eq!(plan.shell, Shell::PowerShell);
        assert!(plan.script.starts_with("$ErrorActionPreference = 'Stop'\n"));
        let remove = plan.script.find("Remove-DnsClientNrptRule").unwrap();
        let add = plan.script.find("Add-DnsClientNrptRule -Namespace '.doze' -NameServers '127.0.0.53' -Comment 'doze zone'").unwrap();
        assert!(remove < add, "{}", plan.script);
        assert!(
            plan.script.contains("$_.Comment -eq 'doze zone'"),
            "only doze's rule is removed"
        );

        f.rules.push(rule(".doze", "127.0.0.53", ZONE_COMMENT));
        assert!(check(&f).ok());
        assert_eq!(install_plan(&f).unwrap(), None, "in place: nothing to do");

        f.resolver = "127.0.0.1:5323".into();
        assert!(check(&f).steps[0].detail.contains("only to port 53"));
        assert!(
            install_plan(&f).is_err(),
            "no rule to a port the NRPT cannot reach"
        );

        let un = uninstall_plan(&["# cleanup".into()]).script;
        assert!(
            un.contains("Remove-DnsClientNrptRule")
                && un.contains("# cleanup")
                && !un.contains("Add-"),
            "{un}"
        );
    }
}
