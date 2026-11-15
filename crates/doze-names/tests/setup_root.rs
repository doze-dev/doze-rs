//! Setup and trust for real, with the privilege they need: install, check, a name and a declared domain answering
//! through the system's own resolver, the doze CA trusted and untrusted, uninstall. It changes the machine, so it runs
//! only when asked (`DOZE_NAMES_ROOT_E2E=1`) and only for a throwaway `DOZE_HOME`:
//!
//! - Linux: the Lima VM, as the user (sudo once per step, passwordless there): scripts/names-e2e-linux.sh.
//! - Windows: the elevated CI runners (.github/workflows/ci.yml, job `os`).
//! - macOS: the owner's Mac, once: scripts/names-setup-e2e-darwin.sh (keeps Go's daemon and puts it back).

#![cfg(feature = "setup")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use doze_names::ca::{Ca, ca_dir};
use doze_names::setup::trust::{TrustRequest, trust, trust_status, untrust};
use doze_names::setup::{Daemon, How, Options, Os, Setup};
use doze_names::{HELPER_ARG, home};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::net::{IpAddr, ToSocketAddrs};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const NAME: &str = "web.n5e2e.doze";
const DOMAIN: &str = "dev.n5-e2e.test";

fn wanted() -> bool {
    if std::env::var("DOZE_NAMES_ROOT_E2E").as_deref() != Ok("1") {
        eprintln!("setup_root: skipped (DOZE_NAMES_ROOT_E2E=1 runs it; it changes the machine)");
        return false;
    }
    let h = home();
    assert!(
        std::env::var_os("DOZE_HOME").is_some() && h.to_string_lossy().contains("e2e"),
        "a throwaway DOZE_HOME (its path containing \"e2e\") is required, never the real one: {}",
        h.display()
    );
    true
}

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_doze-names"))
}

fn options(out: &mut Vec<u8>) -> Options<'_> {
    Options {
        out,
        how: How::Terminal,
    }
}

/// Holds the name and the domain in a process of its own (which also stands in line for the resolver, as every doze
/// program does), and returns it with the addresses it printed.
fn claim() -> (Child, BTreeMap<String, IpAddr>) {
    let mut child = Command::new(bin())
        .args(["claim", NAME, DOMAIN])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut addrs = BTreeMap::new();
    let out = BufReader::new(child.stdout.take().unwrap());
    for line in out.lines() {
        let line = line.unwrap();
        if line == "ready" {
            break;
        }
        let (host, ip) = line.split_once(' ').unwrap();
        addrs.insert(host.to_string(), ip.parse().unwrap());
    }
    (child, addrs)
}

/// What the system's resolver (getaddrinfo, through whatever routes the zone) says `host` is, once it says `want`.
fn resolves(host: &str, want: IpAddr) -> bool {
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut last = Vec::new();
    while Instant::now() < deadline {
        last = (host, 80)
            .to_socket_addrs()
            .map(|a| a.map(|s| s.ip()).collect())
            .unwrap_or_default();
        if last.contains(&want) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    eprintln!("{host}: the system resolved {last:?}, want {want}");
    false
}

fn resolvectl(host: &str) -> String {
    let out = Command::new("resolvectl")
        .args(["query", host])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr)
}

fn install(s: &Setup) {
    let mut out = Vec::new();
    s.install(&mut options(&mut out)).unwrap();
    eprintln!("{}", String::from_utf8_lossy(&out));
    let st = s.check();
    eprintln!("after install, {}:\n{st}", st.platform);
    assert!(st.ok(), "every part in place after install");
    let mut out = Vec::new();
    s.install(&mut options(&mut out)).unwrap();
    assert!(
        String::from_utf8_lossy(&out).contains("nothing to do") || s.os == Os::Linux,
        "a second install changes nothing"
    );
}

fn trust_and_untrust(os: Os, ip: IpAddr) {
    let h = home();
    let mut req = TrustRequest {
        domains: vec![DOMAIN.into()],
        ca: Some(Ca::load_or_create(&ca_dir(&h)).unwrap()),
        home: Some(h.clone()),
        ..TrustRequest::default()
    };
    let mut out = Vec::new();
    trust(os, &mut options(&mut out), &mut req).unwrap();
    eprintln!("{}", String::from_utf8_lossy(&out));
    for st in trust_status(&req) {
        assert!(st.done, "after trust: {} · {}", st.name, st.detail);
    }
    assert!(resolves(DOMAIN, ip), "the routed domain answers");
    if os == Os::Linux {
        let said = resolvectl(DOMAIN);
        assert!(said.contains(&ip.to_string()), "resolvectl: {said}");
    }
    let ca = req.ca.as_ref().unwrap();
    let bundle = std::fs::read_to_string(ca.bundle().unwrap()).unwrap();
    let root = std::fs::read_to_string(ca.root_path()).unwrap();
    assert!(
        bundle.contains(root.trim()),
        "the bundle carries the doze root"
    );

    let mut out = Vec::new();
    untrust(os, &mut options(&mut out), &req).unwrap();
    for st in trust_status(&req) {
        assert!(!st.done, "after untrust: {} · {}", st.name, st.detail);
    }
}

#[test]
fn setup_and_trust_for_real() {
    if !wanted() {
        return;
    }
    let s = Setup {
        daemon: Daemon {
            program: bin(),
            args: vec![HELPER_ARG.into()],
        },
        ..Setup::native(Daemon::this_program().unwrap())
    };
    install(&s);
    let (mut child, addrs) = claim();
    let ip = addrs[NAME];
    let zone_ok = resolves(NAME, ip);
    if s.os == Os::Linux {
        let said = resolvectl(NAME);
        eprintln!("resolvectl query {NAME}:\n{said}");
        assert!(
            said.contains(&ip.to_string()),
            "systemd-resolved answers the zone"
        );
    }
    trust_and_untrust(s.os, addrs[DOMAIN]);
    let _ = child.kill();
    let _ = child.wait();
    assert!(zone_ok, "the zone answers through the system resolver");

    let mut out = Vec::new();
    s.uninstall(&mut options(&mut out)).unwrap();
    let st = s.check();
    eprintln!("after uninstall, {}:\n{st}", st.platform);
    assert!(!st.ok(), "uninstall took it away");
}
