//! What can be done to one service (Go `instanceCommands`, `clientLine`), each a row with its key and its command
//! line, the most likely first for its state. The rows are tables, kept one to a line (`rustfmt::skip`) so Go's order
//! reads at a glance.

use std::time::SystemTime;

use doze_display::{self as words, Kind};

use super::commands::{cmd, page, verb_on, with_text};
use super::{DoKind, Row, Svc, asks, first_of, key_of, web_url};
use crate::view::StackView;

/// A verb on the service, with its command line; "⏎ asks first" when it asks.
fn verb_row(
    key: &str,
    n: &str,
    (glyph, label): (&'static str, String),
    (verb, cli): (&str, String),
    also: &'static str,
    ask: bool,
) -> Row {
    let mut r = cmd(glyph, label, also, verb_on(key, verb, n, ask));
    r.cli = cli;
    if ask {
        "⏎ asks first".clone_into(&mut r.key);
    }
    r
}

/// Everything a service can have done to it, the most likely first for its state (Go `instanceCommands`).
#[rustfmt::skip]
#[must_use]
pub fn instance_commands(st: &StackView, s: Svc<'_>, _now: SystemTime) -> Vec<Row> {
    let (v, key) = (s.v, key_of(st));
    let n = v.name.as_str();
    let open_page = Row { note: "its page".into(), ..cmd("→", format!("Open {n}"), "go show", page(&key, "instance", n)) };
    let logs = Row { cli: format!("doze logs {n}"), ..cmd("≡", format!("Logs of {n}"), "tail log", page(&key, "logs", n)) };
    if v.kind == Kind::Task {
        let run = verb_row(&key, n, ("▸", format!("Run {n}")), ("task", format!("doze task {n}")), "start", false);
        return vec![run, logs, open_page];
    }
    let mut out = state_verbs(&key, s);
    out.extend(opens(&key, s));
    out.push(logs);
    out.push(Row { note: "how long it slept, what woke it".into(), ..cmd("◷", format!("Activity of {n}"), "timeline history savings", page(&key, "activity", n)) });
    out.extend(copies(&key, s));
    out.extend(keeps(&key, s));
    out.push(open_page);
    if v.kind == Kind::Module {
        let mut r = verb_row(&key, n, ("⚠", format!("Reset {n}…")), ("reset", format!("doze reset {n}")), "wipe", true);
        r.bad = true;
        "deletes its data; asks first".clone_into(&mut r.note);
        out.push(r);
    }
    out
}

/// The verbs its state suggests first: put back, sleep and restart, or wake.
#[rustfmt::skip]
fn state_verbs(key: &str, s: Svc<'_>) -> Vec<Row> {
    let (v, n) = (s.v, s.v.name.as_str());
    if v.disabled {
        return vec![verb_row(key, n, ("↑", format!("Put {n} back in this run")), ("on", format!("doze on {n}")), "on enable", false)];
    }
    if words::awake_now(v) {
        return vec![
            verb_row(key, n, ("↓", format!("Sleep {n}")), ("sleep", format!("doze sleep {n}")), "stop", asks(v, "sleep")),
            verb_row(key, n, ("↻", format!("Restart {n}")), ("restart", format!("doze restart {n}")), "kill reboot", asks(v, "restart")),
        ];
    }
    let mut out = vec![verb_row(key, n, ("↑", format!("Wake {n}")), ("wake", format!("doze wake {n}")), "start", false)];
    if words::troubled(v) {
        out.push(verb_row(key, n, ("↻", format!("Restart {n}")), ("restart", format!("doze restart {n}")), "kill reboot", false));
    }
    out
}

/// Its pages in the browser: its address, a module's console.
#[rustfmt::skip]
fn opens(key: &str, s: Svc<'_>) -> Vec<Row> {
    let (v, n) = (s.v, s.v.name.as_str());
    let mut out = Vec::new();
    let url = first_of(&[s.https, web_url(v)]);
    if !url.is_empty() {
        out.push(Row { note: url.into(), cli: format!("open {url}"), ..cmd("↗", format!("Open {n} in the browser"), "browse web", with_text(DoKind::Open, key, url, n)) });
    }
    let console = first_of(&[&v.console, &v.web_url]);
    if !console.is_empty() && v.kind == Kind::Module {
        out.push(Row { note: console.into(), ..cmd("▣", format!("Open {n}'s console"), "admin ui", with_text(DoKind::Open, key, console, n)) });
    }
    out
}

/// The copy rows: its address, its connection string, as an env line, as a shell client's line.
#[rustfmt::skip]
fn copies(key: &str, s: Svc<'_>) -> Vec<Row> {
    let (v, n) = (s.v, s.v.name.as_str());
    let copy = |label: String, note: &str, also| Row { note: note.to_owned(), ..cmd("⧉", label, also, with_text(DoKind::Copy, key, note, n)) };
    let mut out = Vec::new();
    let line = first_of(&[s.https, &v.url, &v.endpoint]);
    if !line.is_empty() {
        out.push(Row { cli: format!("doze url {n}"), ..copy(format!("Copy {n}'s URL"), line, "address") });
    }
    if !v.url.is_empty() && v.url != first_of(&[s.https, &v.url]) {
        out.push(copy(format!("Copy {n}'s connection string"), &v.url, "dsn url"));
    }
    if !s.env_var.is_empty() && !v.url.is_empty() {
        out.push(copy(format!("Copy as {}", s.env_var), &format!("{}={}", s.env_var, v.url), "env .env"));
    }
    if let Some(cli) = client_line(&v.url) {
        let prog = cli.split_whitespace().next().unwrap_or("");
        out.push(copy(format!("Copy the {prog} line"), &cli, "shell client"));
    }
    out
}

/// Pin or unpin, and take it out of this run.
#[rustfmt::skip]
fn keeps(key: &str, s: Svc<'_>) -> Vec<Row> {
    let (v, n) = (s.v, s.v.name.as_str());
    let mut out = vec![if v.keep_awake {
        verb_row(key, n, ("◆", format!("Unpin {n}")), ("pin", format!("doze pin --off {n}")), "", false)
    } else {
        verb_row(key, n, ("◆", format!("Pin {n} awake")), ("pin", format!("doze pin {n}")), "keep", false)
    }];
    if !v.disabled {
        out.push(verb_row(key, n, ("⊘", format!("Take {n} out of this run")), ("off", format!("doze off {n}")), "off disable", asks(v, "off")));
    }
    out
}

/// The shell client a connection string opens, when doze knows one (Go `clientLine`, with `%q` quoting).
#[must_use]
pub fn client_line(url: &str) -> Option<String> {
    let q = |s: &str| serde_json::to_string(s).unwrap_or_default();
    if url.starts_with("postgres://") || url.starts_with("postgresql://") {
        Some(format!("psql {}", q(url)))
    } else if url.starts_with("redis://") || url.starts_with("rediss://") {
        Some(format!("redis-cli -u {url}"))
    } else if url.starts_with("mongodb://") {
        Some(format!("mongosh {}", q(url)))
    } else if url.starts_with("mysql://") {
        Some(format!("mysql --uri {}", q(url)))
    } else {
        None
    }
}
