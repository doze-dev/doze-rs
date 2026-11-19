//! `up`'s trust hint (Go `trustHint`): one line after the boots when an app's domains are not set up on this machine
//! yet (the stack runs either way, on its doze names), and one naming routes no stack declares any more. Nothing is
//! said for a stack without domains.

use crate::paint::Paint;
use doze_config::Config;
use doze_names::ca::{Ca, ca_dir};
use doze_names::domains::route_domains;
use doze_names::routes::Routes;
use doze_names::setup::Os;
use doze_names::setup::trust::{TrustRequest, trust_status};
use std::path::{Path, PathBuf};

/// The hint's lines, for stdout.
pub(crate) fn lines(cfg: &Config, p: Paint) -> Vec<String> {
    let enabled = || cfg.instances.iter().filter(|i| i.enabled);
    let domains: Vec<String> = enabled().flat_map(|i| i.domains.clone()).collect();
    if domains.is_empty() {
        return Vec::new();
    }
    let routes = route_domains(&domains).join(", ");
    let mark = p.dim("›");
    if !matches!(Os::native(), Os::Mac | Os::Linux) {
        return vec![format!(
            "{mark} {routes}: domains and https are not set up on this platform yet; the app answers at its doze name"
        )];
    }
    let home = Path::new(&cfg.home);
    let mut out = Vec::new();
    let mut req = TrustRequest {
        domains: domains.clone(),
        ca: None,
        prune: Vec::new(),
        home: Some(PathBuf::from(home)),
        untrust_all: false,
    };
    // The daemon made the authority as it came up (a domain without the person's own certificate); up only looks.
    if enabled().any(|i| !i.domains.is_empty() && i.cert.is_empty()) {
        match Ca::probe(&ca_dir(home)) {
            Ok(ca) => req.ca = ca,
            Err(e) => out.push(format!(
                "{mark} the certificate authority cannot be used: {e}"
            )),
        }
    }
    let trust = format!("{}{}", p.bold("doze trust"), p.dim(" (one sudo)"));
    if trust_status(&req).iter().any(|s| !s.done) {
        let verb = if route_domains(&domains).len() > 1 {
            "need"
        } else {
            "needs"
        };
        out.push(format!(
            "{mark} {routes} {verb} a one-time setup here: {trust}"
        ));
    }
    let stale = Routes::open(home).stale();
    if !stale.is_empty() {
        out.push(format!(
            "{mark} no stack declares {} any more: {}{}",
            stale.join(", "),
            p.bold("doze trust"),
            p.dim(" removes the route")
        ));
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use doze_config::Instance;

    fn cfg(domains: &[&str], enabled: bool) -> Config {
        let home = std::env::temp_dir().join(format!("dz-hint-{}", std::process::id()));
        Config {
            home: home.display().to_string(),
            instances: vec![Instance {
                name: "web".into(),
                domains: domains.iter().map(|d| (*d).to_owned()).collect(),
                enabled,
                ..Instance::default()
            }],
            ..Config::default()
        }
    }

    #[test]
    fn says_nothing_without_domains_and_names_the_setup_with_them() {
        let p = Paint::new(false);
        assert!(lines(&cfg(&[], true), p).is_empty());
        assert!(
            lines(&cfg(&["shop.hint-test"], false), p).is_empty(),
            "an app turned off"
        );
        let got = lines(&cfg(&["shop.hint-test", "*.shop.hint-test"], true), p);
        let want = if matches!(Os::native(), Os::Mac | Os::Linux) {
            "› shop.hint-test needs a one-time setup here: doze trust (one sudo)"
        } else {
            "› shop.hint-test: domains and https are not set up on this platform yet; the app answers at its doze name"
        };
        assert_eq!(got[0], want, "{got:?}");
    }
}
