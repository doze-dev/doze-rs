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

/// The stacks whose tab was closed (`closedTabs`; Go kept `doze.closedTabs` in the page's storage): still running,
/// listed in the Library, back in the strip when opened again. By the stack's folder, as `last` names it.
pub(crate) fn closed_tabs() -> Vec<String> {
    closed_of(&load())
}

fn closed_of(m: &serde_json::Map<String, Value>) -> Vec<String> {
    m.get("closedTabs")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// Keeps the closed tabs.
pub(crate) fn save_closed_tabs(keys: &[String]) {
    save(|m| {
        m.insert(
            "closedTabs".into(),
            Value::Array(keys.iter().cloned().map(Value::String).collect()),
        );
    });
}

#[cfg(test)]
mod tests {
    use super::closed_of;
    use serde_json::json;

    #[test]
    fn closed_tabs_read_as_strings_and_anything_else_reads_as_none() {
        let m = json!({"closedTabs": ["/code/shop", 3, "/code/api"]});
        assert_eq!(
            closed_of(m.as_object().unwrap()),
            vec!["/code/shop".to_owned(), "/code/api".to_owned()]
        );
        let bad = json!({"closedTabs": "x"});
        assert!(closed_of(bad.as_object().unwrap()).is_empty());
    }
}
