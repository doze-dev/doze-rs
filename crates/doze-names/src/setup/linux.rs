//! Linux setup. There is no /etc/resolver here, so this is four problems wearing one coat: the resolver's interface,
//! the route to it (systemd-resolved or dnsmasq), the hosts block, and the sysctl for the front door's :80; plus the
//! daemon that answers the zone when no doze program does.
//!
//! Which DNS manager owns the machine is read from resolv.conf's marker comment ([`crate::resolvconf`]). Plain
//! resolv.conf cannot do split DNS at all: the only way to serve one zone from there is to become the machine's resolver
//! for everything, which a development tool has no business doing. On those machines apex names come from /etc/hosts
//! and per-stack names do not resolve, said plainly rather than worked around.

use super::hosts::{apex_block, block_installed, replace_block};
use super::linux_units::{
    NETD_UNIT_NAME, UNPRIVILEGED_PORT_START, ZONE_UNIT_NAME, dnsmasq_text, netd_serves, netd_stamp,
    netd_unit, sysctl_text, zone_unit,
};
use super::run::shell_quote;
use super::who::job_outdated;
use super::{Options, Plan, Setup, Shell, Status, Step};
use crate::resolvconf::{Manager, can_route_domain, detect};
use crate::{Error, RESOLVER_IP, RESOLVER_PORT, SUFFIX};
use std::fmt::Write as _;
use std::path::Path;
use std::process::Command;

/// What setup found on the machine: everything the check and the script depend on, so both are functions of it.
#[derive(Debug, Clone)]
pub(crate) struct Facts {
    pub(crate) manager: Manager,
    /// dnsmasq is running (not merely installed).
    pub(crate) dnsmasq: bool,
    /// The hosts file as it is now.
    pub(crate) hosts: Option<String>,
}

impl Facts {
    pub(crate) fn read(s: &Setup) -> Facts {
        Facts {
            manager: detect(&s.paths.resolv_conf),
            dnsmasq: dnsmasq_running(&s.paths.dnsmasq_dir),
            hosts: std::fs::read_to_string(&s.paths.hosts).ok(),
        }
    }

    fn routable(&self) -> bool {
        can_route_domain(self.manager, self.dnsmasq)
    }
}

fn active(unit: &str) -> bool {
    Command::new("systemctl")
        .args(["is-active", unit])
        .output()
        .is_ok_and(|o| o.status.success() && String::from_utf8_lossy(&o.stdout).trim() == "active")
}

/// Whether dnsmasq is RUNNING, not merely installed. Installed-but-idle is common (it arrives as a dependency of other
/// packages), and writing a drop-in for a daemon nobody uses produces a config that does nothing while the report
/// claims the route is in place: a false success, worse than an honest "not available".
fn dnsmasq_running(dir: &Path) -> bool {
    if !dir.is_dir() || doze_os::which::which("dnsmasq").is_none() {
        return false;
    }
    // No systemd, or a dnsmasq started some other way: ask whether the process is there at all.
    active("dnsmasq")
        || Command::new("pgrep")
            .args(["-x", "dnsmasq"])
            .output()
            .is_ok_and(|o| o.status.success())
}

fn sysctl_applied(s: &Setup) -> bool {
    std::fs::read_to_string(&s.paths.port_start)
        .ok()
        .and_then(|t| t.trim().parse::<u32>().ok())
        .is_some_and(|start| start <= UNPRIVILEGED_PORT_START)
}

/// Mirrors what the install writes, and has to keep mirroring it: both routes need the interface unit; resolved needs
/// nothing else, dnsmasq its drop-in as well. (Checking by manager alone reported a route missing on a machine where it
/// had just been written: a plain resolv.conf with dnsmasq running, which is Colima.)
fn route_installed(s: &Setup, m: Manager) -> bool {
    let Ok(unit) = std::fs::read_to_string(&s.paths.zone_unit) else {
        return false;
    };
    if !unit.contains(RESOLVER_IP) {
        return false;
    }
    if m == Manager::Resolved {
        return unit.contains("resolvectl");
    }
    std::fs::read_to_string(s.paths.dnsmasq_dir.join("doze.conf"))
        .is_ok_and(|d| d.contains(RESOLVER_IP))
}

/// The daemon's line: in place only when the unit is this build's, serves this person's registry, and runs.
fn netd_step(s: &Setup) -> Step {
    let unit = std::fs::read_to_string(&s.paths.netd_unit).ok();
    let registry = s.registry().display().to_string();
    let (done, detail) = match unit.as_deref() {
        None => (false, "not installed — a routed domain is unreachable while no stack runs".to_string()),
        Some(_) if job_outdated(&s.paths.netd_unit, &netd_stamp()) => (
            false,
            "from an older setup — run setup again (one sudo), so a routed domain is answered when no stack runs".into(),
        ),
        // Set up under a sudo that swapped HOME for root's: the daemon answers from a registry nobody writes.
        Some(u) if netd_serves(u).is_some_and(|r| r != registry) => (
            false,
            format!("answers from {}, not your registry — run setup again (one sudo)", netd_serves(u).unwrap_or_default()),
        ),
        Some(_) if !active(NETD_UNIT_NAME) => {
            (false, format!("installed but not running (systemctl status {NETD_UNIT_NAME})"))
        }
        Some(_) => (true, "answers the zone when no doze program runs, so a routed domain is never dark".into()),
    };
    Step::new("network daemon", done, detail)
}

pub(crate) fn check(s: &Setup, f: &Facts) -> Status {
    let sysctl = sysctl_applied(s);
    let mut steps = vec![Step::new(
        "unprivileged ports",
        sysctl,
        if sysctl {
            format!("ports ≥ {UNPRIVILEGED_PORT_START} bindable unprivileged")
        } else {
            format!(
                "not applied — services cannot bind :80 (the resolver is unaffected: it uses {RESOLVER_PORT})"
            )
        },
    )];
    let hosts = block_installed(&s.paths.hosts);
    steps.push(Step::new(
        "hosts block",
        hosts,
        if hosts {
            format!("apex names in {}", s.paths.hosts.display())
        } else {
            format!("missing — aws.{SUFFIX} will not resolve")
        },
    ));
    // The route is a step only where it is achievable: listing it as failed on a machine that structurally cannot do
    // split DNS would be reporting the OS as broken.
    if f.routable() {
        let routed = route_installed(s, f.manager);
        steps.push(Step::new(
            "resolver route",
            routed,
            if routed {
                format!("per-stack names routed to {}", crate::resolver_addr())
            } else {
                "not configured — only apex names will resolve".to_string()
            },
        ));
        steps.push(netd_step(s));
    } else {
        steps.push(Step::new(
            "resolver route",
            true,
            format!(
                "n/a on {} — apex names via {}, per-stack names unavailable",
                f.manager,
                s.paths.hosts.display()
            ),
        ));
    }
    Status {
        platform: format!("linux ({})", f.manager),
        steps,
    }
}

fn q(p: &Path) -> String {
    shell_quote(&p.display().to_string())
}

/// Writes `text` to `path` whole (so re-running cannot grow it), through printf's `%s` so no `%` or backslash in it is
/// read as a format (Go wrote the text as the format itself).
fn write_line(b: &mut String, text: &str, path: &Path) {
    let _ = writeln!(b, "printf '%s' {} > {}", shell_quote(text), q(path));
}

pub(crate) fn install_plan(s: &Setup, f: &Facts) -> Result<Plan, Error> {
    let hosts = f
        .hosts
        .as_deref()
        .ok_or_else(|| Error::Setup(format!("cannot read {}", s.paths.hosts.display())))?;
    let p = &s.paths;
    let mut b = String::from("set -e\n");
    // mkdir first, and the whole write allowed to fail: debian:stable-slim has no /etc/sysctl.d at all, so the write
    // died with "Directory nonexistent" and `set -e` took the install (and a doze-aws container) with it.
    let sysctl_dir = q(p.sysctl.parent().unwrap_or(Path::new("/")));
    let _ = writeln!(
        b,
        "mkdir -p {sysctl_dir} && printf '%s' {} > {} || true",
        shell_quote(&sysctl_text()),
        q(&p.sysctl)
    );
    // Our own file only, and allowed to fail. `sysctl --system` reapplies EVERY sysctl.d file; inside a container most
    // of /proc/sys is read-only, so it failed on keys that have nothing to do with doze and `set -e` aborted before the
    // hosts block was written. This key is the one optional step: it lets an unprivileged process bind :80, a container
    // runs as root anyway, and everything downstream copes when it is not set.
    let _ = writeln!(b, "sysctl -q -p {} || true", q(&p.sysctl));
    write_line(&mut b, &replace_block(hosts, &apex_block()), &p.hosts);
    if f.routable() {
        route_lines(s, f, &mut b)?;
    }
    let mut what = format!("apply the sysctl, write the {} block", p.hosts.display());
    if f.routable() {
        what.push_str(&format!(
            ", route .{SUFFIX} to the resolver, and install the daemon that answers it when no stack runs"
        ));
    }
    Ok(Plan {
        what,
        shell: Shell::Sh,
        script: b,
    })
}

/// The resolver's interface (on both routes: it is where the resolver binds, not only what resolved is pointed at),
/// dnsmasq's drop-in where that is the route, and the daemon: a copy of the program, run as the person who ran setup,
/// restarted so re-running setup puts the new copy to work.
fn route_lines(s: &Setup, f: &Facts, b: &mut String) -> Result<(), Error> {
    let (uid, gid) = s.person()?;
    let p = &s.paths;
    write_line(b, &zone_unit(f.manager == Manager::Resolved), &p.zone_unit);
    let _ = writeln!(
        b,
        "systemctl daemon-reload && systemctl enable --now {ZONE_UNIT_NAME}"
    );
    let _ = writeln!(b, "systemctl restart {ZONE_UNIT_NAME}");
    if f.manager != Manager::Resolved {
        let _ = writeln!(b, "mkdir -p {}", q(&p.dnsmasq_dir));
        write_line(b, &dnsmasq_text(), &p.dnsmasq_dir.join("doze.conf"));
        b.push_str("systemctl restart dnsmasq 2>/dev/null || true\n");
    }
    let prog = q(&p.netd_program);
    let _ = writeln!(
        b,
        "mkdir -p {} && cp -f {} {prog} && chown root:root {prog} && chmod 755 {prog}",
        q(p.netd_program.parent().unwrap_or(Path::new("/"))),
        q(&s.daemon.program),
    );
    write_line(b, &netd_unit(s, uid, gid), &p.netd_unit);
    let _ = writeln!(
        b,
        "systemctl daemon-reload && systemctl enable --now {NETD_UNIT_NAME} && systemctl restart {NETD_UNIT_NAME}"
    );
    Ok(())
}

pub(crate) fn after_install(s: &Setup, o: &mut Options<'_>) -> Result<(), Error> {
    let f = Facts::read(s);
    if !f.routable() {
        writeln!(
            o.out,
            "\nnote: {} cannot route a single domain, so per-stack names (<service>.<stack>.{SUFFIX})\n\
             will not resolve here. Apex names work via {}. Installing systemd-resolved or\n\
             dnsmasq would enable the rest; doze will not take over your machine's DNS to do it.",
            f.manager,
            s.paths.hosts.display()
        )?;
    }
    Ok(())
}

pub(crate) fn uninstall_plan(s: &Setup, cleanup: &[String]) -> Result<Plan, Error> {
    let p = &s.paths;
    let hosts = std::fs::read_to_string(&p.hosts)
        .map_err(|e| Error::Setup(format!("read {}: {e}", p.hosts.display())))?;
    let mut b = String::from("set -e\n");
    let _ = writeln!(
        b,
        "systemctl disable --now {NETD_UNIT_NAME} 2>/dev/null || true"
    );
    let _ = writeln!(
        b,
        "systemctl disable --now {ZONE_UNIT_NAME} 2>/dev/null || true"
    );
    let _ = writeln!(
        b,
        "ip link del {} 2>/dev/null || true",
        crate::RESOLVER_IFACE
    );
    let files = [
        &p.sysctl,
        &p.zone_unit,
        &p.dnsmasq_dir.join("doze.conf"),
        &p.netd_unit,
        &p.netd_program,
    ];
    let files: Vec<String> = files.iter().map(|f| q(f)).collect();
    let _ = writeln!(b, "rm -f {}", files.join(" "));
    b.push_str("systemctl daemon-reload 2>/dev/null || true\n");
    write_line(&mut b, &replace_block(&hosts, ""), &p.hosts);
    b.push_str("sysctl -q --system 2>/dev/null || true\n");
    // Restart both: the uninstall removes whichever drop-in was written, and the daemon keeps serving the old config
    // until it re-reads it. dnsmasq pointing .doze at a resolver that is gone is worse than never configured.
    b.push_str("systemctl restart systemd-resolved 2>/dev/null || true\n");
    b.push_str("systemctl restart dnsmasq 2>/dev/null || true\n");
    // The domains' routes drop-in, and the authority's trust.
    let _ = writeln!(
        b,
        "rm -rf {}",
        shell_quote(&format!("{}.d", p.zone_unit.display()))
    );
    for line in cleanup {
        b.push_str(line);
        b.push('\n');
    }
    Ok(Plan {
        what: "remove the sysctl, the hosts block, the resolver route, the domains' routes and the certificate authority"
            .into(),
        shell: Shell::Sh,
        script: b,
    })
}

#[cfg(test)]
#[path = "linux_tests.rs"]
mod tests;
