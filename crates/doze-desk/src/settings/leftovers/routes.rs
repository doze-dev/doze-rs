//! The routes record (`<home>/routes.json`, the names crate's: domain → the project folders that declared it) and
//! the script that removes a route's resolver file. A domain reaches a privileged script, so only a valid one gets
//! through, and it is quoted anyway.

use std::collections::BTreeMap;
use std::path::Path;

/// The record, valid domains only (Go names `readRoutes`): a key that is not a valid domain, or a wildcard, is dropped,
/// since the file is the person's to edit.
pub fn read_routes(home: &Path) -> BTreeMap<String, Vec<String>> {
    std::fs::read_to_string(home.join("routes.json"))
        .map(|t| parse_routes(&t))
        .unwrap_or_default()
}

/// The record's text read.
// fuzz:parser fuzz_routes
pub fn parse_routes(text: &str) -> BTreeMap<String, Vec<String>> {
    let mut m: BTreeMap<String, Vec<String>> = serde_json::from_str(text).unwrap_or_default();
    m.retain(|d, _| valid_domain(d));
    m
}

/// A domain doze routes: letters, digits and hyphens in dot-separated labels of 1–63, at most 253 in all, at least
/// two labels; no wildcard (Go names `ValidDomain`, `Wildcard`).
pub fn valid_domain(d: &str) -> bool {
    let labels: Vec<&str> = d.split('.').collect();
    d.len() <= 253
        && labels.len() >= 2
        && labels.iter().all(|l| {
            !l.is_empty()
                && l.len() <= 63
                && !l.starts_with('-')
                && !l.ends_with('-')
                && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
}

/// The script that removes the routes' resolver files: only valid domains, each quoted.
pub fn route_script(domains: &[String]) -> String {
    domains
        .iter()
        .filter(|d| valid_domain(d))
        .map(|d| {
            format!(
                "rm -f {}",
                super::super::quote(&format!("/etc/resolver/{d}"))
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Forgets routes that are gone from the record, unless a project still declares them (Go names `RouteRemoved`).
/// Written whole, through a temporary file renamed over it.
pub fn forget_routes(home: &Path, gone: &[String]) -> std::io::Result<()> {
    let path = home.join("routes.json");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(());
    };
    let mut m: BTreeMap<String, Vec<String>> = serde_json::from_str(&text).unwrap_or_default();
    m.retain(|d, dirs| {
        !gone.contains(d) || dirs.iter().any(|x| Path::new(x).join("doze.hcl").is_file())
    });
    let body = serde_json::to_string_pretty(&m).map_err(std::io::Error::other)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, body + "\n")?;
    std::fs::rename(&tmp, &path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_valid_domains_reach_the_script() {
        assert!(valid_domain("shop.test"));
        assert!(valid_domain("a-b.dev.acme.com"));
        for bad in [
            "",
            "test",
            "*.shop.test",
            "shop..test",
            "-x.test",
            "x-.test",
            "a b.test",
            "x.test;rm",
            "x/../y.test",
        ] {
            assert!(!valid_domain(bad), "{bad}");
        }
        let m = parse_routes(r#"{"shop.test":["/a"],"*.x.test":["/b"],"bad name":[]}"#);
        assert_eq!(m.keys().collect::<Vec<_>>(), ["shop.test"]);
        assert!(parse_routes("{").is_empty());
        let s = route_script(&["shop.test".into(), "x;y".into()]);
        assert_eq!(s, "rm -f '/etc/resolver/shop.test'");
    }

    #[test]
    fn a_route_still_declared_is_kept() {
        let home = tempfile::tempdir().unwrap();
        let live = home.path().join("live");
        std::fs::create_dir_all(&live).unwrap();
        std::fs::write(live.join("doze.hcl"), "").unwrap();
        let rec = serde_json::json!({"gone.test": ["/nowhere"], "live.test": [live]});
        std::fs::write(home.path().join("routes.json"), rec.to_string()).unwrap();
        forget_routes(home.path(), &["gone.test".into(), "live.test".into()]).unwrap();
        let after = read_routes(home.path());
        assert_eq!(after.keys().collect::<Vec<_>>(), ["live.test"]);
    }
}
