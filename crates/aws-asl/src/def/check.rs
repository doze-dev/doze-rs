//! The checks that need the whole machine: transitions, reachability, a terminal state, the rules of `Retry` and `Catch`, and
//! that every path and payload template reads. AWS's order, as recorded: `StartAt`, then each state in document order (its own
//! problems, then its branches'), then the unreachable states, then the missing terminal state.

use std::collections::HashSet;

use serde_json::Value;

use super::fields::SCHEMA;
use super::{
    Catcher, Definition, Diagnostic, Field, Kind, Lang, Retrier, State, Test, TimeSource, Wait,
};
use crate::errors::{ALL, RESERVED};

const TARGET: &str = "MISSING_TRANSITION_TARGET";

pub(super) fn machine(def: &Definition, at: &str, top: bool, out: &mut Vec<Diagnostic>) {
    let has = |name: &str| def.state(name).is_some();
    if !def.start_at.is_empty() && !has(&def.start_at) {
        out.push(Diagnostic::error(
            TARGET,
            format!("Missing 'Next' target: {}", def.start_at),
            &format!("{at}/StartAt"),
        ));
    }
    for s in &def.states {
        let here = format!(
            "{at}/States/{}",
            s.name.replace('~', "~0").replace('/', "~1")
        );
        state(def, s, &here, out);
    }
    if !def.start_at.is_empty() {
        let reached = reachable(def);
        for s in &def.states {
            if !reached.contains(s.name.as_str()) {
                out.push(Diagnostic::error(
                    TARGET,
                    format!("State \"{}\" is not reachable.", s.name),
                    &format!("{at}/States/{}", s.name),
                ));
            }
        }
    }
    if !def.states.is_empty() && !def.states.iter().any(State::terminal) {
        out.push(Diagnostic {
            code: "MISSING_END_STATE",
            message: "Workflow has no terminal state".to_owned(),
            // Recorded for the machine itself; for a branch it is from memory.
            location: if top { None } else { Some(at.to_owned()) },
            warning: false,
        });
    }
}

fn reachable(def: &Definition) -> HashSet<&str> {
    let mut seen = HashSet::new();
    let mut todo = vec![def.start_at.as_str()];
    while let Some(name) = todo.pop() {
        let Some(s) = def.state(name) else { continue };
        if !seen.insert(s.name.as_str()) {
            continue;
        }
        todo.extend(s.next.as_deref());
        todo.extend(s.catch.iter().map(|c| c.next.as_str()));
        if let Kind::Choice { rules, default } = &s.kind {
            todo.extend(rules.iter().filter_map(|r| r.next.as_deref()));
            todo.extend(default.as_deref());
        }
    }
    seen
}

fn target(def: &Definition, name: &str, at: String, out: &mut Vec<Diagnostic>) {
    if def.state(name).is_none() {
        out.push(Diagnostic::error(
            TARGET,
            format!("Missing 'Next' target: {name}"),
            &at,
        ));
    }
}

fn state(def: &Definition, s: &State, at: &str, out: &mut Vec<Diagnostic>) {
    if s.unknown {
        return;
    }
    let flows = !matches!(s.kind, Kind::Choice { .. } | Kind::Succeed | Kind::Fail(_));
    if flows {
        match (&s.next, s.end) {
            // Both recorded 2026-11-15.
            (Some(_), true) => out.push(Diagnostic::error(
                SCHEMA,
                "There should only be one of the following fields: [Next, End]",
                at,
            )),
            (None, false) => out.push(Diagnostic::error(
                SCHEMA,
                "This object should have only one of the following fields: [Next, End]",
                at,
            )),
            (Some(n), false) => target(def, n, format!("{at}/Next"), out),
            (None, true) => {}
        }
    }
    for (i, c) in s.catch.iter().enumerate() {
        target(def, &c.next, format!("{at}/Catch/{i}/Next"), out);
    }
    if s.lang == Lang::JsonPath {
        needs_parameters(s, at, out);
    }
    errors(&s.retry, &s.catch, at, out);
    if s.lang == Lang::JsonPath {
        paths(s, at, out);
    } else {
        expressions(s, at, out);
    }
    match &s.kind {
        Kind::Choice { rules, default } => {
            for (i, r) in rules.iter().enumerate() {
                if let Some(n) = &r.next {
                    target(def, n, format!("{at}/Choices/{i}/Next"), out);
                }
                if s.lang == Lang::JsonPath {
                    test_paths(&r.test, &format!("{at}/Choices/{i}"), out);
                }
            }
            if let Some(d) = default {
                target(def, d, format!("{at}/Default"), out);
            }
        }
        Kind::Parallel { branches } => {
            for (i, b) in branches.iter().enumerate() {
                machine(b, &format!("{at}/Branches/{i}"), false, out);
            }
        }
        Kind::Map(m) => machine(&m.processor, &format!("{at}/ItemProcessor"), false, out),
        Kind::Wait(Wait::Timestamp(TimeSource::Fixed(t))) if crate::time::parse(t).is_none() => {
            out.push(Diagnostic::error(
                SCHEMA,
                "The value for the field 'Timestamp' must be a valid ISO-8601 timestamp", // (memory)
                &format!("{at}/Timestamp"),
            ));
        }
        _ => {}
    }
}

/// An optimized integration says what to send: without `Parameters` it is refused (recorded 2026-11-15 for `lambda:invoke`; the
/// AWS SDK integrations, whose calls may take nothing, are left alone).
fn needs_parameters(s: &State, at: &str, out: &mut Vec<Diagnostic>) {
    if let Kind::Task(t) = &s.kind
        && s.parameters.is_none()
        && t.resource.starts_with("arn:aws:states:::")
        && !t.resource.starts_with("arn:aws:states:::aws-sdk:")
    {
        out.push(Diagnostic::error(
            SCHEMA,
            format!(
                "Parameters field is required for resource ARN: {}",
                t.resource
            ),
            &format!("{at}/Resource"),
        ));
    }
}

fn errors(retry: &[Retrier], catch: &[Catcher], at: &str, out: &mut Vec<Diagnostic>) {
    let lists = retry
        .iter()
        .map(|r| ("Retry", &r.errors, retry.len()))
        .enumerate()
        .chain(
            catch
                .iter()
                .map(|c| ("Catch", &c.errors, catch.len()))
                .enumerate(),
        );
    for (i, (field, names, len)) in lists {
        let here = format!("{at}/{field}[{i}]/ErrorEquals");
        if names.is_empty() {
            out.push(Diagnostic::error(
                SCHEMA,
                "The field 'ErrorEquals' must not be empty",
                &here,
            )); // (memory)
        }
        if let Some(k) = names.iter().position(|n| n == ALL)
            && (names.len() > 1 || i + 1 != len)
        {
            // Recorded 2026-11-15, with the place the name stands.
            out.push(Diagnostic::error(
                SCHEMA,
                "States.ALL must appear alone and at end of list",
                &format!("{here}[{k}]"),
            ));
        }
        for n in names
            .iter()
            .filter(|n| n.starts_with("States.") && !RESERVED.contains(&n.as_str()))
        {
            // (memory)
            out.push(Diagnostic::error(
                SCHEMA,
                format!("{n} is not a predefined error name"),
                &here,
            ));
        }
    }
}

fn bad_path(name: &str, at: &str, reference: Option<&str>, out: &mut Vec<Diagnostic>) {
    let message = match reference {
        Some(p) => reference_reason(p),
        // (memory)
        None => format!("The value for the field '{name}' must be a valid JSONPath"),
    };
    out.push(Diagnostic::error(SCHEMA, message, &format!("{at}/{name}")));
}

/// Why `p` is not a reference path, as AWS says it: a bracket must open a quoted name or an index (recorded 2026-11-15 for
/// `$.a[*]`); anything else in the general words (memory).
fn reference_reason(p: &str) -> String {
    let bracket = p
        .match_indices('[')
        .any(|(i, _)| !p[i + 1..].starts_with(|c: char| c == '\'' || c.is_ascii_digit()));
    if bracket {
        "Value is not a Reference Path: '[' must be followed by ' or '['".to_owned()
    } else {
        "Value is not a Reference Path".to_owned()
    }
}

fn paths(s: &State, at: &str, out: &mut Vec<Diagnostic>) {
    for (name, f) in [("InputPath", &s.input_path), ("OutputPath", &s.output_path)] {
        if let Field::Text(p) = f
            && !crate::path::is_path(p)
        {
            bad_path(name, at, None, out);
        }
    }
    if let Field::Text(p) = &s.result_path
        && !crate::path::is_reference_path(p)
    {
        bad_path("ResultPath", at, Some(p), out);
    }
    if let Kind::Map(m) = &s.kind
        && let Field::Text(p) = &m.items_path
        && !crate::path::is_path(p)
    {
        bad_path("ItemsPath", at, None, out);
    }
    for (name, tpl) in [
        ("Parameters", &s.parameters),
        ("ResultSelector", &s.result_selector),
        ("Assign", &s.assign),
    ] {
        if let Some(t) = tpl {
            template(t, &format!("{at}/{name}"), out);
        }
    }
    if let Kind::Map(m) = &s.kind
        && let Some(t) = &m.item_selector
    {
        template(t, &format!("{at}/ItemSelector"), out);
    }
}

/// Every `.$` member of a payload template must hold a path or an intrinsic call.
fn template(t: &Value, at: &str, out: &mut Vec<Diagnostic>) {
    match t {
        Value::Object(m) => {
            for (k, v) in m {
                let here = format!("{at}/{k}");
                if k.ends_with(".$") {
                    let ok = v
                        .as_str()
                        .is_some_and(|e| crate::path::is_path(e) || crate::intrinsic::is_call(e));
                    if !ok {
                        // (memory)
                        out.push(Diagnostic::error(
                            SCHEMA,
                            format!("The value for the field '{k}' must be a valid JSONPath or a valid intrinsic function call"),
                            &here,
                        ));
                    }
                } else {
                    template(v, &here, out);
                }
            }
        }
        Value::Array(a) => a
            .iter()
            .enumerate()
            .for_each(|(i, v)| template(v, &format!("{at}/{i}"), out)),
        _ => {}
    }
}

fn test_paths(t: &Test, at: &str, out: &mut Vec<Diagnostic>) {
    match t {
        Test::And(all) | Test::Or(all) => all.iter().for_each(|t| test_paths(t, at, out)),
        Test::Not(t) => test_paths(t, at, out),
        Test::Compare { variable, .. } if !crate::path::is_path(variable) => {
            bad_path("Variable", at, None, out);
        }
        _ => {}
    }
}

/// Every `{% %}` string of a JSONata state must compile.
fn expressions(s: &State, at: &str, out: &mut Vec<Diagnostic>) {
    if !crate::jsonata::available() {
        out.push(Diagnostic::error(
            SCHEMA,
            "JSONata is not built into this doze-aws (it was built without the jsonata feature)",
            at,
        ));
        return;
    }
    let mut found = Vec::new();
    for (name, v) in [
        ("Arguments", &s.arguments),
        ("Output", &s.output),
        ("Assign", &s.assign),
    ] {
        if let Some(v) = v {
            crate::jsonata::problems(v, &format!("{at}/{name}"), &mut found);
        }
    }
    for (at, why) in found {
        out.push(Diagnostic::error(SCHEMA, why, &at));
    }
}
