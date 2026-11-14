//! The files Linux setup writes, as text: the zone's interface unit, the daemon's unit, the sysctl and dnsmasq
//! drop-ins.

use super::Setup;
use crate::netd::VERSION_LINUX;
use crate::{RESOLVER_IFACE, RESOLVER_IP, RESOLVER_PORT, SUFFIX};

pub(crate) const ZONE_UNIT_NAME: &str = "doze-names.service";
pub(crate) const NETD_UNIT_NAME: &str = "doze-netd.service";
/// The domains' routes: a drop-in of the zone unit, so a reboot sets them again.
pub(crate) const ROUTES_DROP_IN: &str = "/etc/systemd/system/doze-names.service.d/domains.conf";

/// What the sysctl must allow: 80, for the shared front door. The resolver does not need it: it sits on a high port,
/// because 53 is held on the wildcard by whatever already serves DNS.
pub(crate) const UNPRIVILEGED_PORT_START: u32 = 80;

/// The daemon unit's version stamp.
pub(crate) fn netd_stamp() -> String {
    format!("Environment=DOZE_NETD_VERSION={VERSION_LINUX}")
}

/// The unit that makes the resolver's interface and, on a systemd-resolved machine, points resolved at it.
///
/// A unit rather than a config file for two reasons. The interface has to exist at boot, before anything asks for a
/// name. And resolved only takes a per-link server: a global `DNS=` in resolved.conf.d is parsed and then silently
/// dropped when it names a loopback address, which is what Go's first implementation did and why it never worked.
/// resolvectl writes runtime state that does not survive a reboot, so the unit applies it every time.
///
/// Every ExecStart is prefixed "-": re-running setup must not fail because the link already exists.
pub(crate) fn zone_unit(route_resolved: bool) -> String {
    let mut u = format!(
        "[Unit]
Description=doze: the .{SUFFIX} name zone
After=systemd-resolved.service
Wants=systemd-resolved.service

[Service]
Type=oneshot
RemainAfterExit=yes
ExecStart=-/sbin/ip link add {RESOLVER_IFACE} type dummy
ExecStart=-/sbin/ip addr add {RESOLVER_IP}/32 dev {RESOLVER_IFACE}
ExecStart=-/sbin/ip link set {RESOLVER_IFACE} up
"
    );
    if route_resolved {
        u.push_str(&format!(
            "ExecStart=-/usr/bin/resolvectl dns {RESOLVER_IFACE} {RESOLVER_IP}:{RESOLVER_PORT}
ExecStart=-/usr/bin/resolvectl domain {RESOLVER_IFACE} ~{SUFFIX}
"
        ));
    }
    u.push_str(&format!(
        "ExecStop=-/sbin/ip link del {RESOLVER_IFACE}

[Install]
WantedBy=multi-user.target
"
    ));
    u
}

/// The daemon's unit: runs the copy as the person who ran setup, after the zone unit has made the interface the
/// resolver binds, and keeps it running.
pub(crate) fn netd_unit(s: &Setup, uid: u32, gid: u32) -> String {
    let mut exec = vec![s.paths.netd_program.display().to_string()];
    exec.extend(s.daemon.args.iter().cloned());
    exec.push(s.registry().display().to_string());
    format!(
        "[Unit]
Description=doze: answers the .{SUFFIX} zone when no doze program does
After={ZONE_UNIT_NAME}
Wants={ZONE_UNIT_NAME}

[Service]
User={uid}
Group={gid}
{stamp}
ExecStart={exec}
Restart=always
RestartSec=2

[Install]
WantedBy=multi-user.target
",
        stamp = netd_stamp(),
        exec = exec.join(" "),
    )
}

/// The registry an installed daemon unit serves: the last word of its ExecStart line.
// fuzz:parser fuzz_setup
pub fn netd_serves(unit: &str) -> Option<&str> {
    unit.lines()
        .find_map(|l| l.strip_prefix("ExecStart="))
        .and_then(|exec| exec.split_whitespace().last())
}

pub(crate) fn sysctl_text() -> String {
    format!(
        "# doze: lets the shared front door bind :80 unprivileged\nnet.ipv4.ip_unprivileged_port_start={UNPRIVILEGED_PORT_START}\n"
    )
}

pub(crate) fn dnsmasq_text() -> String {
    format!("server=/{SUFFIX}/{RESOLVER_IP}#{RESOLVER_PORT}\n")
}
