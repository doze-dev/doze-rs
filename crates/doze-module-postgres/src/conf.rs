//! What doze writes into the cluster on every boot: `doze.conf` (its tuning and the person's settings), included once
//! from `postgresql.conf`, and a `pg_hba.conf` that trusts the socket only.

use crate::spec::PostgresSpec;
use doze_module::{Result, help};
use std::path::Path;

/// Settings doze decides: where the server listens and keeps its data. A person's value for one is refused at its
/// line (Go dropped it without a word).
pub(crate) const OWNED: [&str; 5] = [
    "listen_addresses",
    "port",
    "unix_socket_directories",
    "hba_file",
    "data_directory",
];

const HEADER: &str = "# Managed by doze — do not edit. Regenerated on every boot.\n";
const INCLUDE: &str = "include = 'doze.conf'";
pub(crate) const HBA: &str = "# Managed by doze — local trust only.
# TYPE  DATABASE  USER  ADDRESS  METHOD
local   all       all            trust
";

/// A value as postgresql.conf reads it: in single quotes, a quote doubled, and a backslash doubled (the file's
/// parser reads `\` as an escape; Go wrote it bare, so `C:\new` lost its backslash).
pub(crate) fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\\', "\\\\").replace('\'', "''"))
}

fn on_off(b: bool) -> &'static str {
    if b { "on" } else { "off" }
}

/// `doze.conf`: the typed tuning, then the person's settings sorted (they win over the tuning, as in Go), then
/// `listen_addresses = ''` last so nothing turns TCP on. `check` has refused owned keys, keys that differ only in case,
/// and anything that would not stay one line.
pub(crate) fn doze_conf(s: &PostgresSpec) -> String {
    let buffers = if s.shared_buffers.is_empty() {
        "16MB"
    } else {
        &s.shared_buffers
    };
    let conns = if s.max_connections == 0 {
        50
    } else {
        s.max_connections
    };
    let mut lines = vec![
        ("shared_buffers".to_string(), quote(buffers)),
        ("max_connections".into(), conns.to_string()),
        ("fsync".into(), on_off(s.fsync).into()),
        ("autovacuum".into(), on_off(s.autovacuum).into()),
        // The dev profile: no point being synchronous without fsync.
        ("synchronous_commit".into(), on_off(s.fsync).into()),
        ("full_page_writes".into(), on_off(s.fsync).into()),
    ];
    for (k, v) in help::settings(&s.settings, &OWNED) {
        lines.push((k.to_string(), quote(v)));
    }
    lines.push(("listen_addresses".into(), quote("")));
    let mut out = HEADER.to_string();
    for (k, v) in lines {
        out.push_str(&format!("{k} = {v}\n"));
    }
    out
}

/// Writes doze.conf and pg_hba.conf, and makes sure postgresql.conf includes doze.conf (once; the rest of that file is
/// the cluster's own).
pub(crate) fn write(pgdata: &Path, s: &PostgresSpec) -> Result<()> {
    let conf = pgdata.join("doze.conf");
    doze_os::fs::write_private(&conf, doze_conf(s).as_bytes())
        .map_err(help::io_error("writing", &conf))?;
    let main = pgdata.join("postgresql.conf");
    let text = std::fs::read_to_string(&main).map_err(help::io_error("reading", &main))?;
    if !text.lines().any(|l| l.trim() == INCLUDE) {
        let joined = format!("{text}\n# Added by doze\n{INCLUDE}\n");
        doze_os::fs::write_private(&main, joined.as_bytes())
            .map_err(help::io_error("writing", &main))?;
    }
    let hba = pgdata.join("pg_hba.conf");
    doze_os::fs::write_private(&hba, HBA.as_bytes()).map_err(help::io_error("writing", &hba))
}

/// A setting name postgresql.conf takes on one line: a letter or `_`, then letters, digits, `_`, `.` or `$`.
pub(crate) fn setting_name(k: &str) -> bool {
    let mut b = k.bytes();
    b.next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == b'_')
        && b.all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'.' | b'$'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_are_quoted_for_the_file() {
        assert_eq!(quote("it's C:\\x"), "'it''s C:\\\\x'");
        assert!(
            setting_name("log_min_duration_statement")
                && setting_name("auto_explain.log_min_duration")
        );
        assert!(
            !setting_name("a b")
                && !setting_name("1x")
                && !setting_name("")
                && !setting_name("x\ny")
        );
    }
}
