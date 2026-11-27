//! `up`'s trust hint (Go `trustHint`): one line after the boots when an app's domains are not set up on this machine
//! yet (the stack runs either way, on its doze names), and one naming routes no stack declares any more. Nothing is
//! said for a stack without domains, except the https line: a stack whose apps have `.doze` web names on a machine
//! that does not trust https for them yet gets one line naming the one-password fix (`up` and `status` both say it),
//! since a refused `https://` says nothing at the client.

use crate::paint::Paint;
use doze_config::{Config, Kind};
use doze_names::ca::{Ca, ca_dir};
use doze_names::domains::route_domains;
use doze_names::routes::Routes;
use doze_names::setup::Os;
use doze_names::setup::trust::{TrustRequest, trust_status};
use std::path::{Path, PathBuf};

/// Whether the stack has `.doze` web names: names on, and an app or service with a port (what the front door serves).
fn web_names(cfg: &Config) -> bool {
    cfg.defaults.names
        && cfg
            .instances
            .iter()
            .any(|i| i.enabled && i.port > 0 && matches!(i.kind(), Kind::App | Kind::Service))
}

/// One line when the stack has web names and this machine does not trust https for them yet: none otherwise, and none
/// where setup does not run. Looks only (the authority's files and the system's trust), never makes anything.
pub(crate) fn https_line(cfg: &Config, home: &Path, p: Paint) -> Option<String> {
    if !web_names(cfg)
        || !matches!(Os::native(), Os::Mac | Os::Linux)
        || doze_doctor::zone_off(std::env::var(doze_names::ENV_ZONE).ok().as_deref())
    {
        return None;
    }
    let why = match Ca::probe(&ca_dir(home)) {
        Ok(Some(ca)) if doze_names::trust_store::ca_trusted(&ca).0 => return None,
        Ok(Some(_)) => "aren't trusted here yet",
        Ok(None) => "are refused here until a one-time setup",
        Err(_) => "are refused: the certificate authority cannot be used",
    };
    let fix = if why.ends_with("cannot be used") {
        doze_doctor::FIX_ROTATE
    } else {
        doze_doctor::FIX_HTTPS
    };
    Some(format!(
        "{} https:// names {why}: {}{}",
        p.dim("›"),
        p.bold(fix),
        p.dim(" (one password)")
    ))
}

/// The hint's lines, for stdout.
pub(crate) fn lines(cfg: &Config, p: Paint) -> Vec<String> {
    let enabled = || cfg.instances.iter().filter(|i| i.enabled);
    let domains: Vec<String> = enabled().flat_map(|i| i.domains.clone()).collect();
    if domains.is_empty() {
        return https_line(cfg, Path::new(&cfg.home), p)
            .into_iter()
            .collect();
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
    } else {
        // The domains are set up: the zone's own names may still lack https (domains that bring their own files).
        out.extend(https_line(cfg, home, p));
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
    fn web_names_without_trusted_https_get_one_line_naming_the_one_password_fix() {
        let p = Paint::new(false);
        let home = std::env::temp_dir().join(format!("dz-hint-https-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let mut c = cfg(&[], true);
        c.home = home.display().to_string();
        c.instances[0].kind = Some(Kind::App);
        c.instances[0].port = 3000;
        assert!(lines(&c, p).is_empty(), "names off: nothing to say");
        c.defaults.names = true;
        let got = lines(&c, p);
        if matches!(Os::native(), Os::Mac | Os::Linux) {
            assert_eq!(
                got,
                [
                    "› https:// names are refused here until a one-time setup: doze doctor --fix (one password)"
                ]
            );
            Ca::load_or_create(&ca_dir(&home)).unwrap();
            assert_eq!(
                lines(&c, p),
                ["› https:// names aren't trusted here yet: doze doctor --fix (one password)"]
            );
        } else {
            assert!(got.is_empty());
        }
        c.instances[0].port = 0;
        assert!(lines(&c, p).is_empty(), "no web name: nothing to say");
        let _ = std::fs::remove_dir_all(&home);
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
