//! A folder with no doze.hcl yet (Go `view/empty.go`, `serve.Empty`, `serve.Setup`, the CLI's `setupFolder`): what doze
//! finds there, the three ways to start (the same three as a bare `doze` in an empty folder), and setting it up the way
//! chosen. Nothing starts until the stack is brought up: a folder set up here is recorded stopped, as `doze up` would
//! record it, so the app shows it and Up is one press.

use std::path::Path;

use doze_display::{self as words, EMPTY_CHOICES, EMPTY_NOTHING_FOUND, STARTER_FILE};

use super::asks::add_dialog;
use super::detect::{Detected, Found};
use super::dialog::Dialog;
use super::stack::Cli;
use crate::StackKey;

/// The page.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Empty {
    pub dir: String,
    pub name: String,
    pub title: String,
    /// "found 2 apps, 1 module · nothing was started", or empty when nothing was recognised.
    pub found: String,
    pub ways: Vec<Way>,
    pub note: String,
    pub items: Vec<Found>,
    /// The dialog "Add one thing" opens.
    pub add: Dialog,
}

/// One way to start.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Way {
    pub n: usize,
    /// `init`, `add` or `empty`.
    pub action: &'static str,
    pub title: &'static str,
    pub what: &'static str,
    /// It cannot work here: set up from the repo when nothing in it is recognised.
    pub off: bool,
}

/// The page for `dir`, given what was found there.
pub fn empty_of(dir: &Path, found: &Detected) -> Empty {
    let shown = dir.display().to_string();
    let name = dir
        .file_name()
        .map_or_else(|| shown.clone(), |n| n.to_string_lossy().into_owned());
    let counts = found.counts();
    let ways = EMPTY_CHOICES
        .iter()
        .zip(["init", "add", "empty"])
        .enumerate()
        .map(|(i, (c, action))| {
            let off = i == 0 && counts.is_empty();
            Way {
                n: i + 1,
                action,
                title: c.title,
                what: if off { EMPTY_NOTHING_FOUND } else { c.what },
                off,
            }
        })
        .collect();
    Empty {
        title: format!(
            "No doze.hcl in {} yet.",
            words::tilde(&shown, words::home().as_deref())
        ),
        found: if counts.is_empty() {
            String::new()
        } else {
            format!("found {} · nothing was started", counts.join(", "))
        },
        note: "The same three choices as a bare doze in an empty folder.".into(),
        items: found.items(),
        add: add_dialog(&shown),
        ways,
        name,
        dir: shown,
    }
}

/// A folder that is there with no doze.hcl in it.
pub fn no_config(dir: &Path) -> bool {
    dir.is_dir() && !dir.join("doze.hcl").exists()
}

/// Sets `dir` up the way chosen (`init`, `add` with `arg`, `empty`), records it stopped under the CLI's doze home, and
/// says what was done; or says why not.
pub fn setup(cli: &Cli, dir: &Path, way: &str, arg: &str) -> Result<String, String> {
    let shown = words::tilde(&dir.display().to_string(), words::home().as_deref()).into_owned();
    let failed = |e: String| format!("setting up {shown} failed: {e}");
    match way {
        "init" | "add" => {
            let mut args = vec![way.to_owned()];
            if way == "add" {
                if arg.trim().is_empty() {
                    return Err(failed("add what? name a module or a service".into()));
                }
                args.push(arg.trim().to_owned());
            }
            let ran = cli.run(dir, &args);
            if !ran.ok {
                let last = ran
                    .printed
                    .lines()
                    .rev()
                    .find(|l| !l.trim().is_empty())
                    .unwrap_or("it failed");
                return Err(failed(format!("doze {way}: {}", last.trim())));
            }
        }
        "empty" => {
            std::fs::write(dir.join("doze.hcl"), STARTER_FILE).map_err(|e| failed(e.to_string()))?
        }
        other => return Err(failed(format!("no way to start called {other:?}"))),
    }
    if !dir.join("doze.hcl").is_file() {
        return Err(failed("no doze.hcl was written".into()));
    }
    record(&cli.home, dir).map_err(|e| failed(e.to_string()))?;
    Ok(match way {
        "init" => format!("wrote doze.hcl from what {shown} says"),
        "add" => format!("wrote doze.hcl with {} in {shown}", arg.trim()),
        _ => format!("wrote a starter doze.hcl in {shown} — fill it in, then: doze up"),
    })
}

/// Records the stack in `dir` under `home`, stopped, as `doze up` records it (schema 1: its doze.hcl and its name); a
/// record that is there already is left as it is.
pub fn record(home: &Path, dir: &Path) -> std::io::Result<StackKey> {
    let config = dir.join("doze.hcl");
    let key = StackKey::for_config(home, &config);
    if key.record().is_file() {
        return Ok(key);
    }
    let shape = crate::overview::Shape::read(dir, None);
    let name = if shape.stack.is_empty() {
        dir.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    } else {
        shape.stack
    };
    std::fs::create_dir_all(key.dir())?;
    let rec = serde_json::json!({ "schema": 1, "config": config, "name": name });
    std::fs::write(key.record(), rec.to_string())?;
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::{empty_of, no_config, record, setup};
    use crate::act::detect::{Detected, Found};
    use crate::act::stack::Cli;

    #[test]
    fn nothing_found_greys_the_first_way_out() {
        let dir = tempfile::tempdir().unwrap();
        let e = empty_of(dir.path(), &Detected::default());
        assert!(e.found.is_empty());
        let first = e.ways.first().unwrap();
        assert!(first.off);
        assert_eq!(
            first.what,
            "nothing here it recognises; add one thing instead"
        );
        assert_eq!(
            e.ways.iter().map(|w| w.action).collect::<Vec<_>>(),
            ["init", "add", "empty"]
        );
        assert_eq!(e.add.title, "Add one thing");
        let found = Detected {
            apps: vec![Found::default()],
            modules: vec![Found::default(), Found::default()],
            ..Detected::default()
        };
        let e = empty_of(dir.path(), &found);
        assert_eq!(e.found, "found 1 app, 2 modules · nothing was started");
        assert!(!e.ways.iter().any(|w| w.off));
        assert!(no_config(dir.path()));
    }

    #[test]
    fn an_empty_file_is_written_and_recorded_stopped() {
        let (dir, home) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let cli = Cli {
            program: "/nonexistent/doze".into(),
            home: home.path().to_path_buf(),
        };
        let said = setup(&cli, dir.path(), "empty", "").unwrap();
        assert!(said.starts_with("wrote a starter doze.hcl in "));
        let key = record(home.path(), dir.path()).unwrap();
        let rec = std::fs::read_to_string(key.record()).unwrap();
        assert!(rec.contains("\"name\":\"my-project\""), "{rec}");
        assert!(setup(&cli, dir.path(), "add", " ").is_err());
        let e = setup(&cli, dir.path(), "init", "").unwrap_err();
        assert!(e.contains("doze init: could not run"), "{e}");
    }
}
