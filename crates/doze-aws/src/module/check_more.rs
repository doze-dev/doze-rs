//! The rest of `check.rs`: rules, secrets, parameters and state machines.

use super::check::{Found, Names, idx, json, key};
use super::spec::AwsSpec;

pub(super) fn rules(s: &AwsSpec, names: &Names, f: &mut Found) {
    for (i, r) in s.rule.iter().enumerate() {
        let here = |k: &str| vec![key("rule"), idx(i), key(k)];
        if r.pattern.is_none() == r.schedule.is_none() {
            f.at(
                vec![key("rule"), idx(i)],
                format!(
                    "rule {:?} needs a pattern or a schedule (one of them)",
                    r.name
                ),
                None,
            );
        }
        if r.schedule.is_some() && r.bus.as_deref().is_some_and(|b| b != "default") {
            f.at(
                here("schedule"),
                "a schedule runs on the default bus only (EventBridge's rule)".into(),
                Some("leave bus out, or use a pattern"),
            );
        }
        json(r.pattern.as_deref(), here("pattern"), f);
        for (j, t) in r.targets.iter().enumerate() {
            let at = [here("targets"), vec![idx(j)]].concat();
            match t.split_once(':') {
                Some((kind @ ("queue" | "topic" | "lambda"), name)) if names.has(kind, name) => {}
                Some((kind @ ("queue" | "topic" | "lambda"), name)) => {
                    f.at(
                        at,
                        format!("no {kind} {name:?} is declared in this block"),
                        None,
                    );
                }
                _ => f.at(
                    at,
                    format!("{t:?} is not a target"),
                    Some("\"queue:name\", \"topic:name\" or \"lambda:name\""),
                ),
            }
        }
    }
}

pub(super) fn rest(s: &AwsSpec, f: &mut Found) {
    for (i, x) in s.secret.iter().enumerate() {
        if x.binary.is_some() {
            f.at(
                vec![key("secret"), idx(i), key("binary")],
                "binary secrets cannot be declared (CloudFormation takes text only)".into(),
                Some("value = \"...\""),
            );
        } else if x.value.is_none() {
            f.at(
                vec![key("secret"), idx(i)],
                format!("secret {:?} needs a value", x.name),
                None,
            );
        }
    }
    for (i, p) in s.parameter.iter().enumerate() {
        if let Some(k) = p
            .kind
            .as_deref()
            .filter(|k| !matches!(*k, "String" | "StringList"))
        {
            f.at(
                vec![key("parameter"), idx(i), key("type")],
                format!("{k:?} is not a parameter type"),
                Some("String or StringList"),
            );
        }
    }
    for (i, m) in s.state_machine.iter().enumerate() {
        if let Some(k) = m
            .kind
            .as_deref()
            .filter(|k| !matches!(k.to_ascii_uppercase().as_str(), "STANDARD" | "EXPRESS"))
        {
            f.at(
                vec![key("state_machine"), idx(i), key("type")],
                format!("{k:?} is not a state machine type"),
                Some("STANDARD or EXPRESS"),
            );
        }
        if m.definition.trim().is_empty() {
            f.at(
                vec![key("state_machine"), idx(i), key("definition")],
                format!("state machine {:?} needs a definition", m.name),
                None,
            );
        }
        json(
            Some(&m.definition),
            vec![key("state_machine"), idx(i), key("definition")],
            f,
        );
    }
}
