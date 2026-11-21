//! What the app reads of the machine's names, read-only: the registry's front door (so an address drops its port while
//! a doze process holds it, Go `urlFor` and `frontedDown`) and whether a stack's own domains are set up here (Go
//! `setupFor` over doze-names' `TrustStatus`: a resolver route each, and the doze authority trusted).
//!
//! Nothing here writes: the registry is read, never pruned (doze-names' own reads prune it); the resolver files and
//! the keychain are asked, never changed. Setting up is `doze trust`, which the page offers to copy.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use crate::view::StackView;

/// The registry's file under the doze home.
const NAMES_FILE: &str = "names.json";
/// The key the front door's holder is kept under (not a name, so it never collides with one).
const INGRESS_KEY: &str = "_ingress";
/// The zone's suffix: names under it are the stack's own (`api.shop.doze`).
const SUFFIX: &str = "doze";

#[derive(Debug, Default, Deserialize)]
struct Entry {
    #[serde(default)]
    pid: i64,
    #[serde(default)]
    target: String,
}

/// The registry's live entries: those whose process still runs.
fn entries(home: &Path) -> BTreeMap<String, Entry> {
    let Ok(raw) = std::fs::read(home.join(NAMES_FILE)) else {
        return BTreeMap::new();
    };
    let mut m: BTreeMap<String, Entry> = serde_json::from_slice(&raw).unwrap_or_default();
    m.retain(|_, e| u32::try_from(e.pid).is_ok_and(doze_os::process::alive));
    m
}

/// The port the front door is held on ("80", or where `DOZE_ZONE_INGRESS` moved it), or `None` when no doze process
/// holds it. (Go `frontDoorPort`.)
#[must_use]
pub fn front_door_port(home: &Path) -> Option<String> {
    let m = entries(home);
    let h = m.get(INGRESS_KEY)?;
    Some(
        h.target
            .rsplit_once(':')
            .map_or_else(|| "80".to_owned(), |(_, p)| p.to_owned()),
    )
}

/// The URL for a `.doze` host as the registry says it works: port-less while the front door is held on 80, with
/// the front door's port when it was moved, else the backend's own port; `None` for a name with no route. (Go:
/// doze-names `URLFor`.)
#[must_use]
pub fn url_for(home: &Path, host: &str) -> Option<String> {
    let m = entries(home);
    let host = host.trim().trim_end_matches('.').to_lowercase();
    let mut h = host.as_str();
    let target = loop {
        if h == SUFFIX {
            return None;
        }
        if let Some(e) = m.get(h) {
            break e.target.clone();
        }
        h = h.split_once('.')?.1;
    };
    if target.is_empty() {
        return None;
    }
    if let Some(front) = m.get(INGRESS_KEY) {
        return Some(match front.target.rsplit_once(':') {
            Some((_, port)) if port != "80" => format!("http://{host}:{port}"),
            _ => format!("http://{host}"),
        });
    }
    let (_, port) = target.rsplit_once(':')?;
    Some(format!("http://{host}:{port}"))
}

/// A stopped stack's http `.doze` address as it will be once up: port-less while the front door is held on 80.
/// (Go `frontedDown`.)
#[must_use]
pub fn fronted_down(address: &str, front: &str) -> Option<String> {
    let rest = address.strip_prefix("http://")?;
    let host = rest.split(':').next().unwrap_or(rest);
    if !host.ends_with(&format!(".{SUFFIX}")) {
        return None;
    }
    Some(if front == "80" {
        format!("http://{host}")
    } else {
        format!("http://{host}:{front}")
    })
}

/// One part of a domain's setup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub name: String,
    pub done: bool,
    pub detail: String,
}

/// The domains a stack declares (Go `view.Domains`).
#[must_use]
pub fn domains(st: &StackView) -> Vec<String> {
    st.instances
        .iter()
        .flat_map(|v| v.domains.iter().cloned())
        .collect()
}

/// Domains folded onto what their wildcards are under, without repeats, sorted (doze-names `RouteDomains`).
#[must_use]
pub fn route_domains(domains: &[String]) -> Vec<String> {
    let mut v: Vec<String> = domains
        .iter()
        .map(|d| {
            let d = d.trim().trim_end_matches('.').to_lowercase();
            d.strip_prefix("*.").map(str::to_owned).unwrap_or(d)
        })
        .collect();
    v.sort();
    v.dedup();
    v
}

/// The resolver doze serves the zone on (`DOZE_ZONE_RESOLVER`, else 127.0.0.1:5323).
fn resolver_addr() -> String {
    std::env::var("DOZE_ZONE_RESOLVER")
        .ok()
        .map(|a| a.trim().to_owned())
        .filter(|a| !a.is_empty())
        .unwrap_or_else(|| "127.0.0.1:5323".to_owned())
}

/// Whether `/etc/resolver/<domain>` sends the domain to doze's resolver (read, never written).
fn routed(domain: &str) -> (bool, String) {
    let file = format!("/etc/resolver/{domain}");
    let addr = resolver_addr();
    let port = addr.rsplit_once(':').map_or("5323", |(_, p)| p);
    match std::fs::read_to_string(&file) {
        Ok(s) if s.contains("127.0.0.1") && s.contains(port) => (true, format!("{file} → {addr}")),
        Ok(_) => (
            false,
            format!(
                "{file} is someone else's and points elsewhere; doze leaves it alone — remove it to let doze route {domain}"
            ),
        ),
        Err(_) => (
            false,
            format!("{file} missing — {domain} will not resolve here"),
        ),
    }
}

/// Whether the doze authority under `home` is trusted here (`security verify-cert`, which asks and changes nothing);
/// `None` when there is no authority yet.
fn ca_trusted(home: &Path) -> Option<(bool, String)> {
    let root = home.join("ca").join("root.crt");
    if !root.is_file() {
        return None;
    }
    let trusted = cfg!(target_os = "macos")
        && std::process::Command::new("security")
            .args(["verify-cert", "-L", "-l", "-q", "-p", "basic", "-c"])
            .arg(&root)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
    Some(if trusted {
        (true, "trusted in the System keychain".to_owned())
    } else {
        (false, format!("not trusted · {}", root.display()))
    })
}

/// What the domains need here, step by step (doze-names `TrustStatus`, without the rotated-out authorities).
#[must_use]
pub fn trust_status(domains: &[String], home: &Path) -> Vec<Step> {
    let mut out: Vec<Step> = route_domains(domains)
        .into_iter()
        .map(|d| {
            let (done, detail) = routed(&d);
            Step {
                name: format!("route {d}"),
                done,
                detail,
            }
        })
        .collect();
    if let Some((done, detail)) = ca_trusted(home) {
        out.push(Step {
            name: "certificate authority".into(),
            done,
            detail,
        });
    }
    out
}

/// The stack page's setup banner (Go `view.Setup`): what is not reachable here yet, what is missing, and the command.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize)]
pub struct Setup {
    pub text: String,
    pub detail: String,
    pub command: String,
    /// The app can ask for the password here, so it offers Set up the machine (Go `Prompt`).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub prompt: bool,
}

/// The banner for a stack whose domains are not all set up here; `None` when it declares none or all is in place.
/// (Go `setupFor`.)
#[must_use]
pub fn setup_of(domains: &[String], steps: &[Step]) -> Option<Setup> {
    if domains.is_empty() {
        return None;
    }
    let missing: Vec<&str> = steps
        .iter()
        .filter(|s| !s.done)
        .map(|s| s.name.as_str())
        .collect();
    if missing.is_empty() {
        return None;
    }
    let routes = route_domains(domains);
    let text = match routes.as_slice() {
        [one] => format!("{one} is not reachable here yet"),
        many => format!("{} are not reachable here yet", many.join(", ")),
    };
    Some(Setup {
        text,
        detail: format!("missing: {} · one admin prompt", missing.join(", ")),
        command: "doze trust".into(),
        prompt: false,
    })
}

#[cfg(test)]
mod tests {
    use super::{Step, fronted_down, route_domains, setup_of};

    #[test]
    fn a_stopped_address_drops_its_port_behind_the_front_door() {
        assert_eq!(
            fronted_down("http://api.shop.doze:8080", "80").as_deref(),
            Some("http://api.shop.doze")
        );
        assert_eq!(
            fronted_down("http://api.shop.doze:8080", "8088").as_deref(),
            Some("http://api.shop.doze:8088")
        );
        assert_eq!(fronted_down("db.shop.doze:5432", "80"), None);
        assert_eq!(fronted_down("https://dev.acme.com", "80"), None);
    }

    #[test]
    fn the_setup_banner_names_the_routes_and_what_is_missing() {
        let d = vec!["*.dev.acme.com".to_owned(), "dev.acme.com".to_owned()];
        assert_eq!(route_domains(&d), vec!["dev.acme.com".to_owned()]);
        let steps = vec![
            Step {
                name: "route dev.acme.com".into(),
                done: false,
                detail: String::new(),
            },
            Step {
                name: "certificate authority".into(),
                done: true,
                detail: String::new(),
            },
        ];
        let s = setup_of(&d, &steps).unwrap_or_default();
        assert_eq!(s.text, "dev.acme.com is not reachable here yet");
        assert_eq!(s.detail, "missing: route dev.acme.com · one admin prompt");
        assert_eq!(s.command, "doze trust");
        assert_eq!(setup_of(&[], &steps), None);
    }
}
