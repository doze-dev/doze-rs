//! What valkey would refuse, or what would quietly break doze, refused before boot with the line in doze.hcl to point
//! at. The Go module passed everything through: a typo in `maxmemory_policy` surfaced as a server that never came up,
//! with the reason in its log.

use crate::ValkeySpec;
use doze_module::help;
use doze_module::proto::msg::error::Diagnostic;
use doze_module::proto::msg::value::PathSeg;

/// The eviction policies of valkey 8 and 9 (src/config.c, `maxmemory_policy_enum`, the same in both).
const POLICIES: &[&str] = &[
    "noeviction",
    "allkeys-lru",
    "allkeys-lfu",
    "allkeys-random",
    "volatile-lru",
    "volatile-lfu",
    "volatile-random",
    "volatile-ttl",
];

/// Settings doze decides: where the server listens and keeps its data, and that it stays in the foreground.
const OWNED: &[&str] = &[
    "port",
    "bind",
    "unixsocket",
    "unixsocketperm",
    "dir",
    "daemonize",
];

/// Settings with a typed argument of their own.
const TYPED: &[(&str, &str)] = &[
    ("maxmemory", "maxmemory"),
    ("maxmemory-policy", "maxmemory_policy"),
    ("appendonly", "appendonly"),
    ("save", "save"),
    ("requirepass", "password"),
];

fn key(k: &str) -> PathSeg {
    PathSeg::Key(k.into())
}

fn at(path: Vec<PathSeg>, message: String, hint: Option<String>) -> Diagnostic {
    Diagnostic {
        path,
        message,
        hint,
    }
}

/// `256mb`, `1gb`, `1048576`: digits and one of valkey's units (src/util.c `memtoull`, any case).
fn is_memory(s: &str) -> bool {
    let digits = s.bytes().take_while(u8::is_ascii_digit).count();
    let unit = s.get(digits..).unwrap_or_default().to_ascii_lowercase();
    digits > 0 && ["", "b", "k", "kb", "m", "mb", "g", "gb"].contains(&unit.as_str())
}

/// `""`, or pairs of `seconds changes`: `900 1 300 10`.
fn is_schedule(s: &str) -> bool {
    let parts: Vec<&str> = s.split_whitespace().collect();
    parts.len().is_multiple_of(2) && parts.iter().all(|p| p.parse::<u64>().is_ok())
}

fn typed(s: &ValkeySpec, out: &mut Vec<Diagnostic>) {
    if let Some(m) = s.maxmemory.as_deref().filter(|m| !is_memory(m)) {
        let hint =
            "a number of bytes, or with a unit: 256mb, 1gb (b, k, kb, m, mb, g, gb)".to_string();
        out.push(at(
            vec![key("maxmemory")],
            format!("{m:?} is not a memory size"),
            Some(hint),
        ));
    }
    if let Some(p) = s.maxmemory_policy.as_deref() {
        out.extend(help::one_of(
            vec![key("maxmemory_policy")],
            "an eviction policy",
            p,
            POLICIES,
        ));
    }
    if let Some(v) = s.save.as_deref().filter(|v| !is_schedule(v)) {
        let hint = "pairs of seconds and changes, e.g. \"900 1 300 10\"; \"\" keeps no snapshots"
            .to_string();
        out.push(at(
            vec![key("save")],
            format!("{v:?} is not a snapshot schedule"),
            Some(hint),
        ));
    }
    if s.password()
        .is_some_and(|p| p.chars().any(char::is_control))
    {
        let why = "a password cannot hold control characters (a newline would end the config line it is written into)";
        out.push(at(vec![key("password")], why.into(), None));
    }
}

fn settings(s: &ValkeySpec, out: &mut Vec<Diagnostic>) {
    for k in s.settings.keys() {
        let lower = k.to_ascii_lowercase();
        let path = vec![key("settings"), key(k)];
        if OWNED.contains(&lower.as_str()) {
            let why = format!(
                "doze sets `{lower}` itself: it is how doze reaches and supervises the server"
            );
            out.push(at(path, why, None));
        } else if let Some((_, arg)) = TYPED.iter().find(|(name, _)| *name == lower) {
            let why = format!("`{lower}` has its own argument");
            out.push(at(path, why, Some(format!("use `{arg}` instead"))));
        }
    }
}

/// Every problem in `s`, each at its place in the block.
pub(crate) fn check(s: &ValkeySpec) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    typed(s, &mut out);
    settings(s, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn spec() -> ValkeySpec {
        ValkeySpec {
            maxmemory: None,
            maxmemory_policy: None,
            appendonly: false,
            password: None,
            save: None,
            settings: BTreeMap::new(),
        }
    }

    #[test]
    fn what_valkey_accepts_passes() {
        let mut s = spec();
        s.maxmemory = Some("256MB".into());
        s.maxmemory_policy = Some("allkeys-lru".into());
        s.save = Some("900 1 300 10".into());
        s.settings.insert("tcp-keepalive".into(), "60".into());
        assert_eq!(check(&s), vec![]);
        s.save = Some(String::new());
        s.maxmemory = Some("0".into());
        assert_eq!(check(&s), vec![]);
    }

    #[test]
    fn each_mistake_points_at_its_line_with_a_way_out() {
        let mut s = spec();
        s.maxmemory = Some("256 megs".into());
        s.maxmemory_policy = Some("allkeys-lur".into());
        s.save = Some("900".into());
        s.settings.insert("Port".into(), "6379".into());
        s.settings.insert("requirepass".into(), "x".into());
        let got = check(&s);
        let paths: Vec<_> = got.iter().map(|d| format!("{:?}", d.path)).collect();
        assert_eq!(got.len(), 5, "{got:#?}");
        let policy = got.iter().find(|d| d.path == vec![key("maxmemory_policy")]);
        assert_eq!(
            policy.and_then(|d| d.hint.as_deref()),
            Some("did you mean \"allkeys-lru\"?"),
            "{paths:?}"
        );
        let pass = got
            .iter()
            .find(|d| d.path == vec![key("settings"), key("requirepass")]);
        assert_eq!(
            pass.and_then(|d| d.hint.as_deref()),
            Some("use `password` instead")
        );
    }
}
