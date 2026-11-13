//! Settings › General's "Reopen the page I had" (`reopen`, on unless turned off, as the Go page's `doze.reopen` was)
//! and where the prefs file is. The page itself is `recall`'s `last` (Go's `doze.last`). The Go app kept both in its
//! web page's storage; the Rust app keeps them in the shared file, under keys the Go app leaves alone.

use serde_json::Value;

use super::{load, save};

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
