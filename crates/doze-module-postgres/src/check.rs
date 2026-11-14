//! What PostgreSQL would refuse at converge, what Go dropped without a word, and what would put lines of its own into
//! doze.conf, refused before boot at its line in doze.hcl.

use crate::names::On;
use crate::spec::{PostgresSpec, some};
use crate::{conf, ext, sql};
use doze_module::help;
use doze_module::proto::msg::error::Diagnostic;
use doze_module::proto::msg::instance::Instance;
use doze_module::proto::msg::value::PathSeg;

fn key(k: &str) -> PathSeg {
    PathSeg::Key(k.into())
}

fn index(i: usize) -> PathSeg {
    PathSeg::Index(u32::try_from(i).unwrap_or(u32::MAX))
}

fn at(path: Vec<PathSeg>, message: String, hint: Option<&str>) -> Diagnostic {
    Diagnostic {
        path,
        message,
        hint: hint.map(str::to_owned),
    }
}

/// The declared major ("17" from "17" or "17.4"); 0 when it does not parse (gates then pass, as Go's did).
pub(crate) fn major(version: &str) -> u32 {
    version
        .split('.')
        .next()
        .and_then(|m| m.trim().parse().ok())
        .unwrap_or(0)
}

/// Everything wrong with the block, each at its path.
pub(crate) fn check(s: &PostgresSpec, inst: &Instance) -> Vec<Diagnostic> {
    let major = major(&inst.version);
    let mut out = Vec::new();
    if s.shared_buffers.chars().any(char::is_control) {
        out.push(at(
            vec![key("shared_buffers")],
            "a size has no line breaks or control characters".into(),
            Some("e.g. \"256MB\""),
        ));
    }
    if s.max_connections < 0 {
        out.push(at(
            vec![key("max_connections")],
            format!("{} connections is not a limit", s.max_connections),
            None,
        ));
    }
    limit(s.connection_limit, vec![key("connection_limit")], &mut out);
    settings(s, major, &mut out);
    roles(s, &mut out);
    for (i, sc) in s.schema.iter().enumerate() {
        if sc.name.is_empty() {
            out.push(at(
                vec![key("schema"), index(i)],
                "schema block is missing a name label".into(),
                None,
            ));
        }
    }
    extensions(s, inst, major, &mut out);
    grants(s, major, &mut out);
    out
}

fn limit(n: Option<i64>, path: Vec<PathSeg>, out: &mut Vec<Diagnostic>) {
    if n.is_some_and(|n| n < -1) {
        out.push(at(
            path,
            "a connection limit is -1 (none) or more".into(),
            None,
        ));
    }
}

/// A setting doze owns, a name or value that would not stay one line of doze.conf, two names PostgreSQL reads as one
/// (its names have no case: Go checked one way and wrote the other, bug 11), and settings newer than the major.
fn settings(s: &PostgresSpec, major: u32, out: &mut Vec<Diagnostic>) {
    let mut seen: Vec<String> = Vec::new();
    for (k, v) in &s.settings {
        let here = vec![key("settings"), key(k)];
        let lower = k.to_ascii_lowercase();
        if conf::OWNED.contains(&lower.as_str()) {
            out.push(at(
                here.clone(),
                format!(
                    "{k} is doze's to set: it decides where the server listens and keeps its data"
                ),
                Some("leave it out; doze url says where the server answers"),
            ));
        }
        if !conf::setting_name(k) || v.chars().any(char::is_control) {
            out.push(at(
                here.clone(),
                format!("{k:?} = {v:?} cannot be one postgresql.conf line"),
                Some("a setting name, and a value without line breaks or control characters"),
            ));
        }
        if seen.contains(&lower) {
            out.push(at(
                here.clone(),
                format!("{k} is set twice: PostgreSQL reads setting names without case"),
                Some("keep one"),
            ));
        }
        seen.push(lower.clone());
        for (gated, since) in [("io_method", 18), ("summarize_wal", 17)] {
            if lower == gated && major != 0 && major < since {
                out.push(at(
                    here.clone(),
                    format!(
                        "settings[{k:?}] requires engine version >= {since} (declared {major})"
                    ),
                    None,
                ));
            }
        }
    }
}

fn roles(s: &PostgresSpec, out: &mut Vec<Diagnostic>) {
    for (i, r) in s.role.iter().enumerate() {
        let here = vec![key("role"), index(i)];
        if r.name.is_empty() {
            out.push(at(
                here.clone(),
                "role block is missing a name label".into(),
                None,
            ));
        } else if r.name == "postgres" || r.name.starts_with("pg_") {
            out.push(at(
                here.clone(),
                format!("role {:?} is PostgreSQL's or doze's own", r.name),
                Some("names starting pg_, and postgres (doze's admin role), are reserved"),
            ));
        }
        if r.member_of.contains(&r.name) {
            out.push(at(
                vec![key("role"), index(i), key("member_of")],
                format!("role {:?} cannot be a member of itself", r.name),
                None,
            ));
        }
        limit(
            r.connection_limit,
            vec![key("role"), index(i), key("connection_limit")],
            out,
        );
        for k in r.config.keys().filter(|k| !conf::setting_name(k)) {
            out.push(at(
                vec![key("role"), index(i), key("config"), key(k)],
                format!("{k:?} is not a setting name"),
                None,
            ));
        }
    }
}

/// Names as their extension (an alias is its extension), each once across the list and the blocks, and each one this
/// major can have ([`ext::refused`]).
fn extensions(s: &PostgresSpec, inst: &Instance, major: u32, out: &mut Vec<Diagnostic>) {
    let listed: Vec<String> = s.extensions.iter().map(|n| ext::canonical(n)).collect();
    let mut blocks: Vec<String> = Vec::new();
    for (i, e) in s.extension.iter().enumerate() {
        let here = vec![key("extension"), index(i)];
        let name = ext::canonical(&e.name);
        if e.name.is_empty() {
            out.push(at(
                here.clone(),
                "extension block is missing a name label".into(),
                None,
            ));
        } else if listed.contains(&name) || blocks.contains(&name) {
            out.push(at(
                here.clone(),
                format!("extension {name:?} is declared more than once"),
                Some("keep the block, or the name in `extensions`"),
            ));
        }
        blocks.push(name);
    }
    for (i, e) in s.exts().iter().enumerate() {
        let path = match s
            .extension
            .iter()
            .position(|b| ext::canonical(&b.name) == e.name)
        {
            Some(b) => vec![key("extension"), index(b)],
            None => vec![
                key("extensions"),
                index(
                    s.extensions
                        .iter()
                        .position(|n| ext::canonical(n) == e.name)
                        .unwrap_or(i),
                ),
            ],
        };
        if let Some((why, hint)) = ext::refused(e, major, &inst.dir) {
            out.push(at(path, why, Some(&hint)));
        }
    }
}

/// Privileges each target takes (PostgreSQL's GRANT): Go checked one list for all, so `EXECUTE ON DATABASE` waited
/// for the server to refuse it.
fn privileges(on: &On, major: u32) -> Vec<&'static str> {
    let mut p = match on {
        On::Database(_) => vec!["CREATE", "CONNECT", "TEMPORARY", "TEMP"],
        On::Schema(_) => vec!["CREATE", "USAGE"],
        On::All("TABLES", _) => {
            let mut t = vec![
                "SELECT",
                "INSERT",
                "UPDATE",
                "DELETE",
                "TRUNCATE",
                "REFERENCES",
                "TRIGGER",
            ];
            if major == 0 || major >= 17 {
                t.push("MAINTAIN");
            }
            t
        }
        On::All("SEQUENCES", _) => vec!["USAGE", "SELECT", "UPDATE"],
        On::All(..) => vec!["EXECUTE"],
    };
    p.extend(["ALL", "ALL PRIVILEGES"]);
    p
}

fn grants(s: &PostgresSpec, major: u32, out: &mut Vec<Diagnostic>) {
    let mut seen: Vec<(String, On)> = Vec::new();
    for (i, g) in s.grant.iter().enumerate() {
        let here = vec![key("grant"), index(i)];
        let loc = format!("grant #{}", i + 1);
        if g.role.is_empty() {
            out.push(at(
                vec![key("grant"), index(i), key("role")],
                format!("{loc}: missing required \"role\""),
                None,
            ));
        }
        if g.privileges.is_empty() {
            out.push(at(
                vec![key("grant"), index(i), key("privileges")],
                format!("{loc}: missing required \"privileges\""),
                None,
            ));
        }
        let (d, sc, obj) = (some(&g.database), some(&g.schema), some(&g.objects));
        let why = match (d, sc, obj) {
            (None, None, _) => Some(format!("{loc}: must target a \"database\" or a \"schema\"")),
            (Some(_), Some(_), _) => Some(format!(
                "{loc}: set either \"database\" or \"schema\", not both"
            )),
            (_, _, Some(o)) if crate::names::kind(o).is_none() => Some(format!(
                "{loc}: invalid objects {o:?} (want tables, sequences, or functions)"
            )),
            (Some(_), None, Some(_)) => Some(format!("{loc}: \"objects\" requires \"schema\"")),
            _ => None,
        };
        if let Some(w) = why {
            out.push(at(here, w, None));
            continue;
        }
        let Some(on) = sql::target(g) else { continue };
        let allowed = privileges(&on, major);
        for (j, p) in g.privileges.iter().enumerate() {
            let norm = p.trim().to_ascii_uppercase();
            let path = vec![key("grant"), index(i), key("privileges"), index(j)];
            if let Some(mut d) = help::one_of(
                path,
                &format!("a privilege on {}", on.sql().to_lowercase()),
                &norm,
                &allowed,
            ) {
                d.message = format!("{loc}: {}", d.message);
                out.push(d);
            }
        }
        if seen.contains(&(g.role.clone(), on.clone())) {
            out.push(at(
                here,
                format!("{loc}: {} is granted twice", on.name(&g.role)),
                Some("one grant per role and target: list every privilege in it"),
            ));
        }
        seen.push((g.role.clone(), on));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn majors_parse_like_go() {
        assert_eq!(major("17"), 17);
        assert_eq!(major("16.14"), 16);
        assert_eq!(major("x"), 0);
    }

    #[test]
    fn each_target_takes_its_own_privileges() {
        let db = privileges(&On::Database("d".into()), 17);
        assert!(db.contains(&"CONNECT") && !db.contains(&"SELECT"));
        assert!(privileges(&On::All("TABLES", "s".into()), 17).contains(&"MAINTAIN"));
        assert!(!privileges(&On::All("TABLES", "s".into()), 16).contains(&"MAINTAIN"));
    }
}
