//! `doze-aws version` and `doze-aws config print`.

use crate::config::{Config, Source, describe, display_path};
use crate::keys::Kind;
use crate::services::ALL;
use crate::toml::quote;

/// `doze-aws version`, `--version`, `-V`.
pub(crate) fn version() -> String {
    let names: Vec<&str> = ALL.iter().map(|s| s.name).collect();
    format!(
        "doze-aws {}\nlocal AWS, {} services: {}\n",
        crate::VERSION,
        names.len(),
        names.join(", ")
    )
}

/// The effective configuration as TOML: each setting with its help above it and where its value came from beside it. Saved as
/// `doze-aws.toml`, it is read back to the same configuration.
pub(crate) fn config(cfg: &Config) -> String {
    let cwd = std::env::current_dir().unwrap_or_default();
    let file = cfg.file.as_ref().map(|f| display_path(f, &cwd));
    let mut out = String::from(
        "# doze-aws: the effective configuration.\n\
         # Each setting is, in order: its flag, its DOZE_AWS_<NAME> variable, this file's key, or its default.\n",
    );
    match &file {
        Some(f) => out.push_str(&format!("# Read from {f}.\n")),
        None => out.push_str("# No config file was read (doze-aws reads ./doze-aws.toml when there is one, or --config FILE).\n"),
    }
    let mut group = "";
    for s in cfg.all() {
        if s.key.group != group {
            group = s.key.group;
            out.push_str(&format!("\n# {group}\n"));
        }
        out.push_str(&format!("\n# {}\n", s.key.help));
        let line = format!("{} = {}", s.key.name, value(s.key.kind, &s.value));
        let from = match &s.source {
            Source::Default => "default".to_owned(),
            other => format!("from {}", describe(s.key, other, file.as_deref())),
        };
        let over = s
            .overruled
            .as_ref()
            .map(|v| format!("; the file says {}", value(s.key.kind, v)))
            .unwrap_or_default();
        out.push_str(&format!("{line:<40} # {from}{over}\n"));
    }
    out
}

fn value(kind: Kind, items: &[String]) -> String {
    match kind {
        Kind::List => format!(
            "[{}]",
            items
                .iter()
                .map(|i| quote(i))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Kind::Count(_) | Kind::Switch => items.concat(),
        Kind::Text | Kind::Choice(_) => quote(&items.concat()),
    }
}
