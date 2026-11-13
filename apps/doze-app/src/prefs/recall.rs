//! What ⌘K and Needs you remember, in the shared prefs file under Go's keys, so the two apps read each other's:
//! `recent` (the services opened lately), `commands` (the commands done from ⌘K), `notNow` (items put aside, by id,
//! with what each said) and `windowClosed` (for "while the window was closed").

use std::collections::BTreeMap;
use std::time::SystemTime;

use doze_desk::palette::{RecentCommand, RecentService, opened as put_first, remember as keep};
use serde_json::Value;

use super::{load, save};

fn list<T: serde::de::DeserializeOwned>(key: &str) -> Vec<T> {
    load()
        .get(key)
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| serde_json::from_value(x.clone()).ok())
                .collect()
        })
        .unwrap_or_default()
}

fn put<T: serde::Serialize>(key: &str, v: &T) {
    if let Ok(v) = serde_json::to_value(v) {
        save(|m| {
            m.insert(key.into(), v);
        });
    }
}

/// The services opened lately, newest first.
pub(crate) fn recent() -> Vec<RecentService> {
    list("recent")
}

/// A service's page was opened: first in the list, once.
pub(crate) fn opened(key: &str, name: &str) {
    let now = recent();
    if now.first().is_some_and(|r| r.key == key && r.name == name) {
        return;
    }
    put("recent", &put_first(&now, key, name));
}

/// The commands done from ⌘K lately, newest first.
pub(crate) fn commands() -> Vec<RecentCommand> {
    list("commands")
}

/// Keeps a command done from ⌘K.
pub(crate) fn remember(c: RecentCommand) {
    put("commands", &keep(&commands(), c));
}

/// The items put aside: id → what each said.
pub(crate) fn not_now() -> BTreeMap<String, String> {
    load()
        .get("notNow")
        .and_then(Value::as_object)
        .map(|o| {
            o.iter()
                .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_owned())))
                .collect()
        })
        .unwrap_or_default()
}

/// Keeps the items put aside.
pub(crate) fn save_not_now(m: &BTreeMap<String, String>) {
    put("notNow", m);
}

/// When the window last closed, once: reading it forgets it, so a reopen does not say it again (Go `Service.Away`).
pub(crate) fn take_window_closed() -> Option<SystemTime> {
    let at = load()
        .get("windowClosed")
        .and_then(Value::as_str)
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(SystemTime::from)?;
    save(|m| {
        m.remove("windowClosed");
    });
    Some(at)
}
