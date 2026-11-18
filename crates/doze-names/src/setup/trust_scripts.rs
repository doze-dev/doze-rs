//! The lines a trust run adds to its one script, per system: routes for declared domains, and the authority trusted or
//! untrusted. Functions of what they are given (whether a route is someone else's is passed in), so every system's
//! lines are tested on every system.

use super::Os;
use super::linux_units::ROUTES_DROP_IN;
use super::run::{ps_quote, shell_quote};
use crate::ca::Ca;
use crate::trust_store::{
    MAC_RESOLVER_DIR, ROUTE_COMMENT, SYSTEM_KEYCHAIN, mac_resolver_port, valid_fingerprint,
};
use crate::{RESOLVER_IFACE, SUFFIX};

/// Whether the system keeps every route in one setting, so each change restates them all (Linux), and the prompt must
/// say so.
pub(crate) fn restates_routes(os: Os) -> bool {
    os == Os::Linux
}

/// Writes a route for each domain in `write` and removes those in `drop`; `keep` is the whole set the machine should
/// hold, which is Linux's concern (one setting). Only doze's own routes are touched: `foreign` says which are not.
/// Domains were validated before they reached a record or a config, and are quoted anyway.
pub(crate) fn routes(
    os: Os,
    keep: &[String],
    write: &[String],
    drop: &[String],
    foreign: &dyn Fn(&str) -> bool,
) -> String {
    let mut lines = Vec::new();
    match os {
        Os::Mac => {
            let port = mac_resolver_port();
            for d in write.iter().filter(|d| !foreign(d)) {
                if lines.is_empty() {
                    lines.push(format!("mkdir -p {MAC_RESOLVER_DIR}"));
                }
                lines.push(format!(
                    "printf 'nameserver 127.0.0.1\\nport {port}\\n' > {}",
                    shell_quote(&format!("{MAC_RESOLVER_DIR}/{d}"))
                ));
            }
            for d in drop.iter().filter(|d| !foreign(d)) {
                lines.push(format!(
                    "rm -f {}",
                    shell_quote(&format!("{MAC_RESOLVER_DIR}/{d}"))
                ));
            }
        }
        Os::Linux => {
            // The link's list holds the zone and every domain: set now (fails harmlessly when the link is not up) and
            // in a drop-in of the zone unit, so a reboot sets it again.
            let list: Vec<String> = std::iter::once(SUFFIX)
                .chain(keep.iter().map(String::as_str))
                .map(|d| format!("~{d}"))
                .collect();
            let list = list.join(" ");
            let unit = format!(
                "[Service]\nExecStart=-/usr/bin/resolvectl domain {RESOLVER_IFACE} {list}\n"
            );
            let dir = ROUTES_DROP_IN.trim_end_matches("/domains.conf");
            lines.push(format!(
                "resolvectl domain {RESOLVER_IFACE} {list} 2>/dev/null || true"
            ));
            lines.push(format!("mkdir -p {}", shell_quote(dir)));
            lines.push(format!(
                "printf '%s' {} > {}",
                shell_quote(&unit),
                shell_quote(ROUTES_DROP_IN)
            ));
            lines.push("systemctl daemon-reload 2>/dev/null || true".into());
        }
        Os::Windows => {
            let addr = crate::resolver_addr();
            let ip = addr.rsplit_once(':').map_or(addr.as_str(), |(h, _)| h);
            for d in write.iter().filter(|d| !foreign(d)) {
                lines.push(remove_route_rule(d));
                lines.push(format!(
                    "Add-DnsClientNrptRule -Namespace {},{} -NameServers {} -Comment {}",
                    ps_quote(d),
                    ps_quote(&format!(".{d}")),
                    ps_quote(ip),
                    ps_quote(ROUTE_COMMENT)
                ));
            }
            for d in drop.iter().filter(|d| !foreign(d)) {
                lines.push(remove_route_rule(d));
            }
        }
        Os::Other => {}
    }
    lines.join("\n")
}

fn remove_route_rule(d: &str) -> String {
    format!(
        "Get-DnsClientNrptRule | Where-Object {{ $_.Comment -eq {} -and $_.Namespace -contains {} }} | ForEach-Object {{ Remove-DnsClientNrptRule -Name $_.Name -Force }}",
        ps_quote(ROUTE_COMMENT),
        ps_quote(&format!(".{d}"))
    )
}

/// Removes every recorded route when the zone itself goes. On Linux the routes are the zone link's and go with it.
pub(crate) fn uninstall_routes(os: Os, recorded: &[String]) -> String {
    if os == Os::Linux {
        return String::new();
    }
    routes(os, &[], &[], recorded, &crate::trust_store::route_foreign)
}

/// Trusts the authority for TLS on this machine. macOS: for TLS and basic chaining only, as mkcert does, never for code
/// signing or mail.
pub(crate) fn trust(os: Os, ca: &Ca) -> String {
    let root = ca.root_path().display().to_string();
    match os {
        Os::Mac => format!(
            "security add-trusted-cert -d -r trustRoot -p ssl -p basic -k {SYSTEM_KEYCHAIN} {}",
            shell_quote(&root)
        ),
        Os::Linux => {
            let (path, update) = crate::trust_store::linux_ca_bundle_path();
            format!(
                "cp -f {} {}\n{update}",
                shell_quote(&root),
                shell_quote(path)
            )
        }
        Os::Windows => format!(
            "certutil.exe -addstore -f Root {}\nif ($LASTEXITCODE) {{ exit $LASTEXITCODE }}",
            ps_quote(&root)
        ),
        Os::Other => String::new(),
    }
}

/// Stops trusting the authority's current root.
pub(crate) fn untrust(os: Os, ca: Option<&Ca>) -> String {
    match os {
        Os::Linux => {
            let (path, update) = crate::trust_store::linux_ca_bundle_path();
            format!("rm -f {}\n{update}", shell_quote(path))
        }
        _ => ca
            .map(|c| untrust_root(os, Some(&c.root_path()), &c.fingerprint()))
            .unwrap_or_default(),
    }
}

/// Stops trusting a rotated-out root: by its kept file where the store wants the certificate (macOS's admin trust),
/// and by fingerprint. Linux's bundle holds one doze root at a time, so a rotated-out root went with the rotation.
pub(crate) fn untrust_fingerprint(os: Os, ca: Option<&Ca>, fp: &str) -> String {
    if os == Os::Linux {
        return String::new();
    }
    let file = ca.map(|c| c.previous_path(fp));
    untrust_root(os, file.as_deref(), fp)
}

/// Removes one root: its admin trust by file (macOS: `delete-certificate -t` touches user trust only, and the root was
/// trusted in the admin store), then the certificate by hash. A fingerprint that is not one never reaches a script.
fn untrust_root(os: Os, file: Option<&std::path::Path>, fp: &str) -> String {
    let mut lines = Vec::new();
    match os {
        Os::Mac => {
            if let Some(f) = file.filter(|f| f.exists()) {
                lines.push(format!(
                    "security remove-trusted-cert -d {} 2>/dev/null || true",
                    shell_quote(&f.display().to_string())
                ));
            }
            if valid_fingerprint(fp) {
                lines.push(format!(
                    "security delete-certificate -t -Z {} {SYSTEM_KEYCHAIN} 2>/dev/null || true",
                    shell_quote(fp)
                ));
            }
        }
        Os::Windows if valid_fingerprint(fp) => lines.push(format!(
            "Get-ChildItem Cert:\\LocalMachine\\Root | Where-Object {{ ([BitConverter]::ToString([Security.Cryptography.SHA256]::Create().ComputeHash($_.RawData)) -replace '-','') -eq {} }} | Remove-Item",
            ps_quote(fp)
        )),
        _ => {}
    }
    lines.join("\n")
}

/// Removes every doze authority the store holds, for when the files that would name them are gone or unreadable.
pub(crate) fn untrust_unknown(os: Os) -> Vec<String> {
    if os == Os::Linux {
        // The file is doze's by name, so no fingerprint is needed.
        return vec![untrust(os, None)];
    }
    crate::trust_store::trusted_doze_roots()
        .iter()
        .map(|fp| untrust_root(os, None, fp))
        .collect()
}
