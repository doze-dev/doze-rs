//! What temporal would refuse, or what doze would get wrong, refused before boot at its line. Go took a bad retention
//! at decode, but a bad restart policy only by its own message, and passed search attributes and dynamic config not at
//! all (it had neither).

use crate::TemporalSpec;
use crate::duration;
use doze_module::help;
use doze_module::proto::msg::error::Diagnostic;
use doze_module::proto::msg::value::PathSeg;

/// The search attribute types the dev server registers (`--search-attribute name=Type`).
pub(crate) const ATTRIBUTE_TYPES: &[&str] = &[
    "Text",
    "Keyword",
    "Int",
    "Double",
    "Bool",
    "Datetime",
    "KeywordList",
];

/// How the supervisor treats an exit (Go's `restart { policy = … }`).
pub(crate) const POLICIES: &[&str] = &["no", "on_failure", "always"];

/// The type as temporal spells it, whatever case it was written in.
pub(crate) fn canonical_type(ty: &str) -> Option<&'static str> {
    ATTRIBUTE_TYPES
        .iter()
        .copied()
        .find(|t| t.eq_ignore_ascii_case(ty))
}

fn key(k: &str) -> PathSeg {
    PathSeg::Key(k.into())
}

fn at(path: Vec<PathSeg>, message: impl Into<String>, hint: Option<&str>) -> Diagnostic {
    Diagnostic {
        path,
        message: message.into(),
        hint: hint.map(String::from),
    }
}

fn indexed(block: &str, i: usize, field: &str) -> Vec<PathSeg> {
    vec![
        key(block),
        PathSeg::Index(u32::try_from(i).unwrap_or(u32::MAX)),
        key(field),
    ]
}

const DURATION_HINT: &str = "a number and a unit: 7d, 168h, 1h30m, 90s (d, h, m, s, ms)";

fn namespaces(s: &TemporalSpec, out: &mut Vec<Diagnostic>) {
    for (i, ns) in s.namespace.iter().enumerate() {
        if let Some(r) = ns.retention.as_deref() {
            match duration::millis(r) {
                None => out.push(at(
                    indexed("namespace", i, "retention"),
                    format!("{r:?} is not a duration"),
                    Some(DURATION_HINT),
                )),
                Some(0) => out.push(at(
                    indexed("namespace", i, "retention"),
                    "retention must be longer than nothing",
                    None,
                )),
                Some(_) => {}
            }
        }
    }
}

fn maps(s: &TemporalSpec, out: &mut Vec<Diagnostic>) {
    for (name, ty) in &s.search_attributes {
        let path = vec![key("search_attributes"), key(name)];
        if name.is_empty() || name.contains('=') || name.chars().any(char::is_whitespace) {
            out.push(at(
                path.clone(),
                format!("{name:?} cannot name a search attribute"),
                None,
            ));
        }
        // temporal reads the type without regard to case (checked on 1.8.2: `keyword` boots); only an unknown one is refused.
        if canonical_type(ty).is_none() {
            out.extend(help::one_of(
                path,
                "a search attribute type",
                ty,
                ATTRIBUTE_TYPES,
            ));
        }
    }
    for k in s.dynamic_config.keys() {
        if k.is_empty() || k.contains('=') || k.chars().any(char::is_whitespace) {
            let path = vec![key("dynamic_config"), key(k)];
            out.push(at(
                path,
                format!("{k:?} cannot name a dynamic config key"),
                None,
            ));
        }
    }
}

fn restart(s: &TemporalSpec, out: &mut Vec<Diagnostic>) {
    if s.restart.len() > 1 {
        out.push(at(
            vec![key("restart"), PathSeg::Index(1)],
            "at most one restart block",
            None,
        ));
    }
    for (i, r) in s.restart.iter().enumerate() {
        if let Some(p) = r.policy.as_deref() {
            out.extend(help::one_of(
                indexed("restart", i, "policy"),
                "a restart policy",
                p,
                POLICIES,
            ));
        }
        if let Some(b) = r
            .backoff
            .as_deref()
            .filter(|b| duration::millis(b).is_none())
        {
            out.push(at(
                indexed("restart", i, "backoff"),
                format!("{b:?} is not a duration"),
                Some(DURATION_HINT),
            ));
        }
    }
}

/// Every problem in `s`, each at its place in the block.
pub(crate) fn check(s: &TemporalSpec) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    if s.ui_port == 0 {
        out.push(at(
            vec![key("ui_port")],
            "the Web UI needs a port",
            Some("leave it out for 8233"),
        ));
    }
    namespaces(s, &mut out);
    maps(s, &mut out);
    restart(s, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Namespace, RestartBlock};
    use std::collections::BTreeMap;

    fn spec() -> TemporalSpec {
        TemporalSpec {
            ui_port: 8233,
            headless: false,
            search_attributes: BTreeMap::from([("CustomerId".into(), "Keyword".into())]),
            dynamic_config: BTreeMap::from([(
                "frontend.enableUpdateWorkflowExecution".into(),
                "true".into(),
            )]),
            namespace: vec![Namespace {
                name: "orders".into(),
                retention: Some("7d".into()),
                description: None,
            }],
            restart: vec![RestartBlock {
                policy: Some("always".into()),
                backoff: Some("2s".into()),
                max_retries: None,
            }],
        }
    }

    #[test]
    fn what_temporal_accepts_passes() {
        assert_eq!(check(&spec()), vec![]);
    }

    #[test]
    fn each_mistake_points_at_its_line_with_a_way_out() {
        let mut s = spec();
        s.namespace[0].retention = Some("seven days".into());
        s.search_attributes.insert("Region".into(), "Keywrd".into());
        s.search_attributes.insert("Plan".into(), "keyword".into());
        s.restart[0].policy = Some("on-failure".into());
        s.restart.push(RestartBlock {
            policy: None,
            backoff: Some("soon".into()),
            max_retries: None,
        });
        let got = check(&s);
        let hint = |path: Vec<PathSeg>| {
            got.iter()
                .find(|d| d.path == path)
                .and_then(|d| d.hint.clone())
        };
        assert_eq!(
            hint(indexed("namespace", 0, "retention")).as_deref(),
            Some(DURATION_HINT)
        );
        assert_eq!(
            hint(vec![key("search_attributes"), key("Region")]).as_deref(),
            Some("did you mean \"Keyword\"?")
        );
        assert_eq!(
            hint(indexed("restart", 0, "policy")).as_deref(),
            Some("did you mean \"on_failure\"?")
        );
        assert!(
            got.iter().any(|d| d.message == "at most one restart block"),
            "{got:#?}"
        );
        assert!(
            got.iter()
                .any(|d| d.path == indexed("restart", 1, "backoff")),
            "{got:#?}"
        );
    }
}
