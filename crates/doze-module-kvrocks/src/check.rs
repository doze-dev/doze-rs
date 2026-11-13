//! What kvrocks would refuse at boot, or would accept and get wrong, refused before boot at its line. Found on kvrocks
//! 2.16.0 itself: it refuses `workers 0` and a namespace token equal to the password, but only at boot, in its log;
//! and it ACCEPTS two namespaces with the same token, after which that token logs into one of them, a silent leak
//! between tenants.

use crate::KvrocksSpec;
use doze_module::proto::msg::error::Diagnostic;
use doze_module::proto::msg::value::PathSeg;
use std::collections::BTreeMap;

/// Settings doze decides: where the server listens and keeps its data, and that it stays in the foreground.
const OWNED: &[&str] = &[
    "dir",
    "bind",
    "port",
    "unixsocket",
    "unixsocketperm",
    "daemonize",
];

/// Settings with an argument or block of their own.
const TYPED: &[(&str, &str)] = &[("requirepass", "`password`"), ("workers", "`workers`")];

fn key(k: &str) -> PathSeg {
    PathSeg::Key(k.into())
}

fn ns_path(i: usize, field: &str) -> Vec<PathSeg> {
    vec![
        key("namespace"),
        PathSeg::Index(u32::try_from(i).unwrap_or(u32::MAX)),
        key(field),
    ]
}

fn at(path: Vec<PathSeg>, message: impl Into<String>, hint: Option<&str>) -> Diagnostic {
    Diagnostic {
        path,
        message: message.into(),
        hint: hint.map(String::from),
    }
}

fn tokens(s: &KvrocksSpec, out: &mut Vec<Diagnostic>) {
    if !s.namespace.is_empty() && s.password().is_none() {
        let why = "kvrocks namespaces require a `password` (the default-namespace token)";
        out.push(at(vec![key("password")], why, None));
    }
    let mut first: BTreeMap<&str, &str> = BTreeMap::new();
    for (i, ns) in s.namespace.iter().enumerate() {
        if Some(ns.token.as_str()) == s.password() {
            let why = "a namespace token cannot be the password: the password is the default namespace's token";
            out.push(at(ns_path(i, "token"), why, None));
        } else if let Some(other) = first.insert(&ns.token, &ns.name) {
            let why = format!(
                "namespace {other:?} has the same token: kvrocks would log both into one of them"
            );
            out.push(at(
                ns_path(i, "token"),
                why,
                Some("give each namespace its own token"),
            ));
        }
    }
}

fn settings(s: &KvrocksSpec, out: &mut Vec<Diagnostic>) {
    for k in s.settings.keys() {
        let lower = k.to_ascii_lowercase();
        let path = vec![key("settings"), key(k)];
        if OWNED.contains(&lower.as_str()) {
            let why = format!(
                "doze sets `{lower}` itself: it is how doze reaches and supervises the server"
            );
            out.push(at(path, why, None));
        } else if let Some((_, arg)) = TYPED.iter().find(|(name, _)| *name == lower) {
            out.push(at(
                path,
                format!("`{lower}` has its own argument"),
                Some(&format!("use {arg} instead")),
            ));
        } else if lower.starts_with("namespace.") {
            let why = "namespaces are declared as blocks, so doze knows their tokens";
            out.push(at(
                path,
                why,
                Some("use a `namespace \"name\" { token = … }` block"),
            ));
        }
    }
}

/// Every problem in `s`, each at its place in the block.
pub(crate) fn check(s: &KvrocksSpec) -> Vec<Diagnostic> {
    let mut out = crate::conf::unwritable(s);
    if s.workers == Some(0) {
        let hint = Some("leave it out for kvrocks's own default");
        out.push(at(
            vec![key("workers")],
            "kvrocks needs at least one worker thread",
            hint,
        ));
    }
    tokens(s, &mut out);
    settings(s, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Namespace;

    fn spec() -> KvrocksSpec {
        KvrocksSpec {
            workers: Some(4),
            password: Some("admin".into()),
            settings: BTreeMap::from([("rocksdb.block_cache_size".into(), "256".into())]),
            namespace: vec![Namespace {
                name: "a".into(),
                token: "tok-a".into(),
            }],
        }
    }

    #[test]
    fn what_kvrocks_accepts_passes() {
        assert_eq!(check(&spec()), vec![]);
    }

    #[test]
    fn each_mistake_points_at_its_line() {
        let mut s = spec();
        s.workers = Some(0);
        s.namespace.push(Namespace {
            name: "b".into(),
            token: "tok-a".into(),
        });
        s.namespace.push(Namespace {
            name: "c".into(),
            token: "admin".into(),
        });
        s.settings.insert("Port".into(), "1".into());
        s.settings.insert("requirepass".into(), "x".into());
        s.settings.insert("namespace.d".into(), "t".into());
        let paths: Vec<Vec<PathSeg>> = check(&s).into_iter().map(|d| d.path).collect();
        assert_eq!(
            paths,
            vec![
                vec![key("workers")],
                ns_path(1, "token"),
                ns_path(2, "token"),
                vec![key("settings"), key("Port")],
                vec![key("settings"), key("namespace.d")],
                vec![key("settings"), key("requirepass")],
            ]
        );
    }
}
