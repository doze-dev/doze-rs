//! macOS: install the network daemon, and route the zone to the resolver over unicast DNS.
//!
//! The daemon gives every service an address of its own (`pool.rs` says why it is a daemon and not a pool of loopback
//! aliases). The unicast route is not optional, and the reason is easy to lose: macOS getaddrinfo drops loopback
//! addresses other than 127.0.0.1 when they are learned over mDNS, as a security guard. So per-service addresses cannot
//! be published over Bonjour: they have to come from a real DNS server, which is what /etc/resolver/doze points at.
//! Anyone reaching for mDNS to avoid the setup step will find names that resolve everywhere except where it matters.

use super::run::shell_quote;
use super::who::job_outdated;
use super::{Options, Plan, Setup, Shell, Status, Step};
use crate::netd::{SERVING_FILE, VERSION_DARWIN};
use crate::pool::Net;
use crate::{Error, SUFFIX};
use std::path::{Path, PathBuf};
use std::time::Duration;

const LABEL: &str = "dev.doze.netd";

fn stamp() -> String {
    format!("<key>DOZE_NETD_VERSION</key><string>{VERSION_DARWIN}</string>")
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// The launchd job: starts the daemon at boot and keeps it running, told the registry to translate from and the user
/// to run as once the interface is up. Its version stamp tells a later setup whether the daemon is this build's.
pub(crate) fn launchd_plist(s: &Setup, uid: u32, gid: u32) -> String {
    let mut args = vec![s.paths.helper.display().to_string()];
    args.extend(s.daemon.args.iter().cloned());
    args.push(s.registry().display().to_string());
    args.push(uid.to_string());
    args.push(gid.to_string());
    let args: String = args
        .iter()
        .map(|a| format!("    <string>{}</string>\n", xml_escape(a)))
        .collect();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{LABEL}</string>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>ThrottleInterval</key><integer>10</integer>
  <key>StandardErrorPath</key><string>{log}</string>
  <key>EnvironmentVariables</key>
  <dict>{stamp}</dict>
  <key>ProgramArguments</key>
  <array>
{args}  </array>
</dict>
</plist>
"#,
        log = xml_escape(&s.paths.helper_log.display().to_string()),
        stamp = stamp(),
    )
}

/// Whether the job at `path` starts a daemon older than this build's.
pub(crate) fn netd_outdated(path: &Path) -> bool {
    job_outdated(path, &stamp())
}

fn resolver_file(s: &Setup) -> PathBuf {
    s.paths.resolver_dir.join(SUFFIX)
}

fn resolver_installed(s: &Setup) -> bool {
    std::fs::read_to_string(resolver_file(s)).is_ok_and(|t| crate::trust_store::mac_points_here(&t))
}

/// Whether a live daemon translates from this zone's registry (the one that wrote its pid beside it), and its interface
/// holds the gateway: binding the gateway address succeeds only when an interface of this machine has it.
fn gateway_up(s: &Setup) -> bool {
    let serves = std::fs::read_to_string(s.home.join(SERVING_FILE))
        .ok()
        .and_then(|t| t.trim().parse::<u32>().ok())
        .is_some_and(doze_os::process::alive);
    serves && std::net::UdpSocket::bind((Net::native().gateway(), 0)).is_ok()
}

pub(crate) fn check(s: &Setup) -> Status {
    let mut up = gateway_up(s);
    let mut detail = format!("{} served by the doze network daemon", Net::native().cidr());
    if !up {
        detail =
            "the doze network daemon is not running — services cannot hold canonical ports".into();
        if s.paths.launchd_job.exists() {
            detail.push_str(&format!(" (see {})", s.paths.helper_log.display()));
        }
    } else if netd_outdated(&s.paths.launchd_job) {
        // What runs is a copy made at setup, so it stays as it was until setup is run again.
        up = false;
        detail = "the doze network daemon is from an older doze — run setup again (one password) for this one".into();
    }
    let file = resolver_file(s);
    let routed = resolver_installed(s);
    let route = if routed {
        format!("{} → {}", file.display(), crate::resolver_addr())
    } else {
        format!("{} missing — .{SUFFIX} will not resolve", file.display())
    };
    Status {
        platform: "darwin".into(),
        steps: vec![
            Step::new("network", up, detail),
            Step::new("resolver route", routed, route),
        ],
    }
}

pub(crate) fn install_plan(s: &Setup) -> Result<Option<Plan>, Error> {
    let (uid, gid) = s.person()?;
    let plist = launchd_plist(s, uid, gid);
    if check(s).ok() && std::fs::read_to_string(&s.paths.launchd_job).is_ok_and(|cur| cur == plist)
    {
        return Ok(None);
    }
    Ok(Some(plan_for(s, &plist)))
}

/// The install script. The daemon is a copy of the program, so it does not vanish when the program is upgraded or
/// moved; bootout before bootstrap, so re-running setup restarts a daemon that is already there on the new copy.
pub(crate) fn plan_for(s: &Setup, plist: &str) -> Plan {
    let p = &s.paths;
    let (job, old, helper) = (q(&p.launchd_job), q(&p.old_launchd_job), q(&p.helper));
    let helper_dir = q(p.helper.parent().unwrap_or(Path::new("/")));
    let resolver_dir = q(&p.resolver_dir);
    let port = crate::trust_store::mac_resolver_port();
    let script = format!(
        "set -e
launchctl bootout system {job} 2>/dev/null || true
launchctl bootout system {old} 2>/dev/null || true
rm -f {old}
mkdir -p {helper_dir}
cp -f {program} {helper}
chown root:wheel {helper}
chmod 755 {helper}
cat > {job} <<'PLIST'
{plist}PLIST
launchctl bootstrap system {job}
mkdir -p {resolver_dir}
printf 'nameserver 127.0.0.1\\nport {port}\\n' > {resolver}
",
        program = shell_quote(&s.daemon.program.display().to_string()),
        resolver = q(&resolver_file(s)),
    );
    Plan {
        what: format!("install the doze network daemon and route .{SUFFIX} to the resolver"),
        shell: Shell::Sh,
        script,
    }
}

fn q(p: &Path) -> String {
    shell_quote(&p.display().to_string())
}

/// launchd starts the daemon asynchronously, so the interface can appear a beat after launchctl returns.
pub(crate) fn after_install(s: &Setup, o: &mut Options<'_>) -> Result<(), Error> {
    for _ in 0..40 {
        if gateway_up(s) {
            break;
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    if !gateway_up(s) {
        return Err(Error::Setup(format!(
            "setup ran but the network daemon did not come up — see {}, then check again",
            s.paths.helper_log.display()
        )));
    }
    writeln!(
        o.out,
        "✓ network daemon running and *.{SUFFIX} routed to the resolver"
    )?;
    Ok(())
}

pub(crate) fn uninstall_plan(s: &Setup, cleanup: &[String]) -> Plan {
    let p = &s.paths;
    let (job, old, helper) = (q(&p.launchd_job), q(&p.old_launchd_job), q(&p.helper));
    let mut script = format!(
        "launchctl bootout system {job} 2>/dev/null || true
launchctl bootout system {old} 2>/dev/null || true
rm -f {job} {old} {helper}
rm -f {}
",
        q(&resolver_file(s))
    );
    for line in cleanup {
        script.push_str(line);
        script.push('\n');
    }
    Plan {
        what: "remove the network daemon, the resolver route, the domains' routes and the certificate authority"
            .into(),
        shell: Shell::Sh,
        script,
    }
}
