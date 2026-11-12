//! What is made of the table: the paths a row is registered under, and `design/routes.json`.

use super::ROUTES;

/// The paths a row is registered under for a prefix: the root is `/` and, under a prefix, both `/k` and `/k/`.
#[must_use]
pub fn paths(base: &crate::base::Base, path: &str) -> Vec<String> {
    if path == "/" && !base.prefix().is_empty() {
        vec![base.prefix().to_owned(), format!("{}/", base.prefix())]
    } else {
        vec![base.url(path)]
    }
}

/// `design/routes.json`: each screen id and the pattern of the route that serves it, sorted by id.
#[must_use]
pub fn json() -> String {
    let mut screens: Vec<(&str, &str)> = ROUTES
        .iter()
        .flat_map(|r| r.screens.iter().map(move |s| (*s, r.path)))
        .collect();
    screens.sort_unstable();
    let mut out = String::from("{\n  \"screens\": {\n");
    for (i, (id, path)) in screens.iter().enumerate() {
        let comma = if i + 1 == screens.len() { "" } else { "," };
        out.push_str(&format!("    \"{id}\": \"{path}\"{comma}\n"));
    }
    out.push_str("  }\n}\n");
    out
}
