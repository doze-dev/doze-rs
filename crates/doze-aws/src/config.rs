//! The effective configuration: each setting from the command line, else the environment (`DOZE_AWS_*`), else `doze-aws.toml`,
//! else its default; with where each value came from, so `config print` and the ready block can say.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::keys::{KEYS, Key, Kind};
use crate::toml;

/// The file read when `--config` does not name one.
pub(crate) const DEFAULT_FILE: &str = "doze-aws.toml";

/// Where a value came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Source {
    /// Nobody set it.
    Default,
    /// The config file, on this line.
    File(usize),
    /// Its `DOZE_AWS_*` variable.
    Env,
    /// Its flag.
    Flag,
}

/// What the command line and the environment gave, by setting.
pub(crate) type Given = BTreeMap<&'static str, (Vec<String>, Source)>;

/// One setting's effective value.
#[derive(Clone, Debug)]
pub(crate) struct Setting {
    /// Which setting.
    pub(crate) key: &'static Key,
    /// The value (one item unless the setting is a list). A path is absolute.
    pub(crate) value: Vec<String>,
    /// Where it came from.
    pub(crate) source: Source,
    /// What the file said, when a flag or variable overrules it.
    pub(crate) overruled: Option<Vec<String>>,
}

/// The effective configuration.
#[derive(Clone, Debug)]
pub(crate) struct Config {
    settings: Vec<Setting>,
    /// The config file read, if one was.
    pub(crate) file: Option<PathBuf>,
}

impl Config {
    /// Resolves every setting. `file` is `--config` (which must exist); without it `./doze-aws.toml` is read if it is there.
    pub(crate) fn load(given: &Given, file: Option<&Path>, cwd: &Path) -> Result<Self, String> {
        let path = match file {
            Some(p) => Some(cwd.join(p)),
            None => Some(cwd.join(DEFAULT_FILE)).filter(|p| p.is_file()),
        };
        let from_file = match &path {
            Some(p) => read_file(p)?,
            None => toml::Entries::new(),
        };
        let file_dir = path
            .as_ref()
            .and_then(|p| p.parent())
            .map_or_else(|| cwd.to_path_buf(), Path::to_path_buf);
        let shown = path.as_ref().map(|p| display_path(p, cwd));
        let mut settings = Vec::with_capacity(KEYS.len());
        for key in KEYS {
            let filed = from_file.get(key.name).map(|(v, line)| (items(v), *line));
            let (mut value, source) = match (given.get(key.name), &filed) {
                (Some((v, s)), _) => (v.clone(), s.clone()),
                (None, Some((v, line))) => (v.clone(), Source::File(*line)),
                (None, None) => (default_of(key), Source::Default),
            };
            let place = describe(key, &source, shown.as_deref());
            check(key, &value).map_err(|e| format!("{place}: {e}"))?;
            let overruled = filed
                .filter(|(v, _)| matches!(source, Source::Flag | Source::Env) && *v != value)
                .map(|(v, _)| v);
            if key.name == "data-dir"
                || (matches!(key.name, "template" | "ca-file") && !value.concat().is_empty())
            {
                let base = if matches!(source, Source::File(_)) {
                    &file_dir
                } else {
                    cwd
                };
                value = vec![clean(&base.join(value.concat()))];
            }
            settings.push(Setting {
                key,
                value,
                source,
                overruled,
            });
        }
        Ok(Self {
            settings,
            file: path,
        })
    }

    /// The setting called `name`.
    pub(crate) fn get(&self, name: &str) -> Option<&Setting> {
        self.settings.iter().find(|s| s.key.name == name)
    }

    /// Every setting, in table order.
    pub(crate) fn all(&self) -> &[Setting] {
        &self.settings
    }

    /// A setting's value as text (a list's items comma-joined).
    pub(crate) fn text(&self, name: &str) -> String {
        self.get(name)
            .map(|s| s.value.join(","))
            .unwrap_or_default()
    }

    /// A list setting's items.
    pub(crate) fn list(&self, name: &str) -> Vec<String> {
        self.get(name).map(|s| s.value.clone()).unwrap_or_default()
    }

    /// A count setting (checked when loaded).
    pub(crate) fn count(&self, name: &str) -> u64 {
        self.text(name).parse().unwrap_or_default()
    }

    /// A switch setting.
    pub(crate) fn switch(&self, name: &str) -> bool {
        self.text(name) == "true"
    }

    /// The data directory, absolute.
    pub(crate) fn data_dir(&self) -> PathBuf {
        PathBuf::from(self.text("data-dir"))
    }

    /// The settings a flag or a variable set over what the file says.
    pub(crate) fn overrules(&self) -> impl Iterator<Item = &Setting> {
        self.settings.iter().filter(|s| s.overruled.is_some())
    }
}

/// Reads the config file, refusing a key no setting has.
fn read_file(path: &Path) -> Result<toml::Entries, String> {
    let shown = path.display();
    let text = std::fs::read_to_string(path).map_err(|e| format!("{shown}: {e}"))?;
    let entries = toml::parse(&text).map_err(|e| format!("{shown}, {e}"))?;
    for (name, (_, line)) in &entries {
        if crate::keys::find(name).is_none() {
            let hint = crate::keys::nearest(name)
                .map_or_else(String::new, |n| format!(" (did you mean `{n}`?)"));
            return Err(format!(
                "{shown}, line {line}: `{name}` is not a setting{hint}; `doze-aws config print` lists them"
            ));
        }
    }
    Ok(entries)
}

fn items(v: &toml::Value) -> Vec<String> {
    match v {
        toml::Value::One(s) => vec![s.clone()],
        toml::Value::Many(v) => v.clone(),
    }
}

fn default_of(key: &Key) -> Vec<String> {
    if key.kind == Kind::List {
        return key
            .default
            .split(',')
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();
    }
    vec![key.default.to_owned()]
}

/// Where a value came from, as a person would look for it.
pub(crate) fn describe(key: &Key, source: &Source, file: Option<&str>) -> String {
    match source {
        Source::Default => format!("the default {}", key.name),
        Source::File(line) => format!(
            "{}, line {line} ({})",
            file.unwrap_or(DEFAULT_FILE),
            key.name
        ),
        Source::Env => crate::keys::env_var(key.name),
        Source::Flag => format!("--{}", key.name),
    }
}

/// A path shown relative to the working directory when it is inside it.
pub(crate) fn display_path(p: &Path, cwd: &Path) -> String {
    p.strip_prefix(cwd).map_or_else(
        |_| p.display().to_string(),
        |r| format!("./{}", r.display()),
    )
}

/// Whether `value` is one `key` takes.
fn check(key: &Key, value: &[String]) -> Result<(), String> {
    let one = value.concat();
    match key.kind {
        Kind::List => return check_list(key, value),
        Kind::Count(min) => match one.parse::<u64>() {
            Ok(n) if n >= min => {}
            _ => return Err(format!("`{one}` is not a whole number of at least {min}")),
        },
        Kind::Switch if one != "true" && one != "false" => {
            return Err(format!("`{one}` is not true or false"));
        }
        Kind::Choice(words) if !words.contains(&one.as_str()) => {
            return Err(format!("`{one}` is not one of {}", words.join(", ")));
        }
        _ => {}
    }
    match key.name {
        "listen" => one
            .parse::<std::net::SocketAddr>()
            .map(|_| ())
            .map_err(|_| format!("`{one}` is not an address (host:port, such as 127.0.0.1:4566)")),
        "region" if !is_region(&one) => {
            Err(format!("`{one}` is not a region name (such as us-east-1)"))
        }
        "account" if one.len() != 12 || !one.bytes().all(|b| b.is_ascii_digit()) => {
            Err(format!("`{one}` is not an account id (twelve digits)"))
        }
        "data-dir" if one.is_empty() => Err("the data directory cannot be empty".into()),
        _ => Ok(()),
    }
}

fn check_list(key: &Key, value: &[String]) -> Result<(), String> {
    if key.name == "services" {
        return crate::services::parse_list(value).map(|_| ());
    }
    Ok(())
}

/// The path without its `.` parts (`/a/./b` is `/a/b`); `..` is kept, as the file system means it.
fn clean(p: &Path) -> String {
    p.components()
        .filter(|c| !matches!(c, std::path::Component::CurDir))
        .collect::<PathBuf>()
        .display()
        .to_string()
}

/// `us-east-1`, `eu-central-2`, `us-gov-west-1`: letters in dashed words, ending in a number.
fn is_region(s: &str) -> bool {
    let mut parts = s.split('-').collect::<Vec<_>>();
    let last = parts.pop().unwrap_or_default();
    parts.len() >= 2
        && !last.is_empty()
        && last.bytes().all(|b| b.is_ascii_digit())
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_lowercase()))
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
