//! Settings › General's switches that are prefs: "Reopen the page I had" (`reopen`, on unless turned off, as the Go
//! page's `doze.reopen` was) and the page itself (`page`: the stack's key and which page). The Go app kept both in
//! its web page's storage; the Rust app keeps them in the shared file, under new keys the Go app leaves alone.

use serde_json::{Value, json};

use super::{load, save};

/// The page the window had, to reopen on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Place {
    /// The stack's key; empty for the Library or Settings.
    pub(crate) key: String,
    /// `library`, `home`, `instance`, `logs`, `activity`, `settings`.
    pub(crate) page: String,
    pub(crate) instance: String,
}

/// Whether the window reopens on the page it had.
pub(crate) fn reopen() -> bool {
    load()
        .get("reopen")
        .and_then(Value::as_bool)
        .unwrap_or(true)
}

/// Keeps the switch.
pub(crate) fn save_reopen(on: bool) {
    save(|m| {
        m.insert("reopen".into(), Value::Bool(on));
    });
}

/// The page kept, if any.
pub(crate) fn place() -> Option<Place> {
    let m = load();
    let p = m.get("page")?;
    let s = |k: &str| p.get(k).and_then(Value::as_str).unwrap_or("").to_owned();
    let page = s("page");
    (!page.is_empty()).then(|| Place {
        key: s("key"),
        page,
        instance: s("instance"),
    })
}

/// Keeps the page shown, unless it is the one kept already.
pub(crate) fn save_place(p: &Place) {
    if place().as_ref() == Some(p) {
        return;
    }
    save(|m| {
        m.insert(
            "page".into(),
            json!({"key": p.key, "page": p.page, "instance": p.instance}),
        );
    });
}

/// Where the file is, as Settings › General shows it: `~/.doze/desk/prefs.json`.
pub(crate) fn shown_path() -> String {
    let p = super::path().map(|p| p.to_string_lossy().into_owned());
    match p {
        Some(p) => {
            let home = std::env::var("HOME").ok();
            doze_display::tilde(&p, home.as_deref()).into_owned()
        }
        None => "~/.doze/desk/prefs.json".into(),
    }
}
