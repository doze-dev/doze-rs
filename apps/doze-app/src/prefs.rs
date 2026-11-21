//! The app's preferences: `$DOZE_HOME/desk/prefs.json`, the Go app's file and keys (Go `internal/desk/prefs`), so
//! moving between the two apps keeps the window's frame and the theme. A write changes only the keys this app owns
//! and keeps every other key as it was. Written atomically (a temporary file renamed over it).
//!
//! Nothing is written until [`set_home`] names the home: a screenshot or a bench run has none.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::SystemTime;

use serde_json::{Map, Value, json};

use crate::theme::{Choice, Mode, Palette};

pub(crate) mod page;
pub(crate) mod recall;

static HOME: OnceLock<PathBuf> = OnceLock::new();

/// The project the window last showed (Go's `lastDir`), written with the frame when the window closes.
static LAST_DIR: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// Notes the project the window shows, for `lastDir`.
pub(crate) fn note_dir(dir: &str) {
    if dir.is_empty() {
        return;
    }
    if let Ok(mut d) = LAST_DIR.lock() {
        *d = Some(dir.to_owned());
    }
}

/// Names the doze home whose prefs this run reads and writes.
pub(crate) fn set_home(home: &Path) {
    let _ = HOME.set(home.to_path_buf());
}

fn path() -> Option<PathBuf> {
    HOME.get().map(|h| h.join("desk").join("prefs.json"))
}

fn load() -> Map<String, Value> {
    path()
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .and_then(|v| match v {
            Value::Object(m) => Some(m),
            _ => None,
        })
        .unwrap_or_default()
}

fn save(edit: impl FnOnce(&mut Map<String, Value>)) {
    let Some(path) = path() else { return };
    let mut m = load();
    edit(&mut m);
    let Ok(mut text) = serde_json::to_string_pretty(&Value::Object(m)) else {
        return;
    };
    text.push('\n');
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, text).is_ok() && std::fs::rename(&tmp, &path).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
}

/// The window's last frame (`window`: Go's `{X, Y, W, H}`), when it was placed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Frame {
    pub(crate) x: i64,
    pub(crate) y: i64,
    pub(crate) w: i64,
    pub(crate) h: i64,
}

fn frame_of(m: &Map<String, Value>) -> Option<Frame> {
    let f = m.get("window")?;
    let n = |k: &str| f.get(k).and_then(Value::as_i64).unwrap_or(0);
    let fr = Frame {
        x: n("X"),
        y: n("Y"),
        w: n("W"),
        h: n("H"),
    };
    (fr.w > 0 && fr.h > 0).then_some(fr)
}

/// The frame the window had when it last closed.
pub(crate) fn frame() -> Option<Frame> {
    frame_of(&load())
}

/// Keeps the frame, the project the window last showed (`lastDir`, when there was one) and the time the window
/// closed (`windowClosed`, for "while the window was closed"), as Go's `WindowClosing` does.
pub(crate) fn save_frame(f: Frame) {
    let dir = LAST_DIR.lock().ok().and_then(|d| d.clone());
    save(|m| {
        if let Some(d) = dir {
            m.insert("lastDir".into(), Value::String(d));
        }
        m.insert(
            "window".into(),
            json!({"X": f.x, "Y": f.y, "W": f.w, "H": f.h}),
        );
        let now: chrono_free::Rfc3339 = SystemTime::now().into();
        m.insert("windowClosed".into(), Value::String(now.0));
    });
}

/// The palette and mode last chosen (`theme`, `mode`).
pub(crate) fn theme() -> Choice {
    let m = load();
    let s = |k: &str| m.get(k).and_then(Value::as_str).unwrap_or("").to_owned();
    Choice {
        palette: Palette::parse(&s("theme")).unwrap_or_default(),
        mode: Mode::parse(&s("mode")).unwrap_or_default(),
    }
}

/// Keeps the palette and mode.
pub(crate) fn save_theme(c: Choice) {
    save(|m| {
        m.insert("theme".into(), Value::String(c.palette.name().into()));
        let mode = match c.mode {
            Mode::System => "system",
            Mode::Dark => "dark",
            Mode::Light => "light",
        };
        m.insert("mode".into(), Value::String(mode.into()));
    });
}

/// The overview's view remembered for a stack (`views`, by the stack's key): `tiles`, `list`, `map`, `grid`.
pub(crate) fn view(key: &str) -> Option<String> {
    load()
        .get("views")
        .and_then(|v| v.get(key))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// Keeps a stack's view.
pub(crate) fn save_view(key: &str, view: &str) {
    save(|m| {
        let views = m.entry("views").or_insert_with(|| json!({}));
        if let Some(o) = views.as_object_mut() {
            o.insert(key.into(), Value::String(view.into()));
        }
    });
}

/// The lists' row height (`density`; the Go app kept it in the page's local storage): `compact`, `cosy`, `roomy`.
pub(crate) fn density() -> String {
    load()
        .get("density")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned()
}

/// Keeps the density.
pub(crate) fn save_density(word: &str) {
    save(|m| {
        m.insert("density".into(), Value::String(word.into()));
    });
}

/// The getting-started strip was put away, and whether a connection was ever seen (`hints`).
pub(crate) fn hints() -> (bool, bool) {
    let m = load();
    let h = m.get("hints");
    let b = |k: &str| {
        h.and_then(|h| h.get(k))
            .and_then(Value::as_bool)
            .unwrap_or(false)
    };
    (b("dismissed"), b("connected"))
}

/// Puts the strip away for good (`hints.dismissed`), or notes the first connection seen (`hints.connected`).
pub(crate) fn save_hint(key: &str) {
    save(|m| {
        let hints = m.entry("hints").or_insert_with(|| json!({}));
        if let Some(o) = hints.as_object_mut() {
            o.insert(key.into(), Value::Bool(true));
        }
    });
}

/// RFC 3339 in UTC from a `SystemTime`, without a date crate (the civil-from-days algorithm).
mod chrono_free {
    use std::time::{SystemTime, UNIX_EPOCH};

    pub(crate) struct Rfc3339(pub String);

    impl From<SystemTime> for Rfc3339 {
        fn from(t: SystemTime) -> Self {
            let secs = t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
            let (days, rem) = (secs / 86_400, secs % 86_400);
            let (y, m, d) = civil(i64::try_from(days).unwrap_or(0));
            Rfc3339(format!(
                "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
                rem / 3600,
                rem % 3600 / 60,
                rem % 60
            ))
        }
    }

    /// Howard Hinnant's days-to-civil.
    fn civil(z: i64) -> (i64, i64, i64) {
        let z = z + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        (yoe + era * 400 + i64::from(m <= 2), m, d)
    }

    #[cfg(test)]
    mod tests {
        use super::Rfc3339;
        use std::time::{Duration, UNIX_EPOCH};

        #[test]
        fn dates_are_civil() {
            let t = UNIX_EPOCH + Duration::from_secs(1_760_191_480);
            assert_eq!(Rfc3339::from(t).0, "2025-10-11T14:04:40Z");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::frame_of;
    use serde_json::json;

    #[test]
    fn the_go_frame_reads_and_an_unplaced_one_does_not() {
        let m = json!({"window": {"X": 10, "Y": 20, "W": 900, "H": 600}, "lastDir": "/x"});
        let f = frame_of(m.as_object().unwrap()).unwrap();
        assert_eq!((f.x, f.y, f.w, f.h), (10, 20, 900, 600));
        let none = json!({"window": {"X": 0, "Y": 0, "W": 0, "H": 0}});
        assert!(frame_of(none.as_object().unwrap()).is_none());
    }
}
