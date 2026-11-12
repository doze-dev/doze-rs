//! One state's JSON to a [`State`].

use serde_json::Value;

use super::fields::{Obj, SCHEMA};
use super::parse::{language, machine};
use super::{
    Catcher, Diagnostic, Fail, Field, Kind, Lang, MapSpec, Retrier, State, Task, TimeSource, Wait,
};

const TYPES: &str = "[Task, Wait, Pass, Succeed, Fail, Choice, Parallel, Map]";

/// Each type's own fields in JSONPath, then in JSONata.
const FIELDS: &[(&str, &[&str], &[&str])] = &[
    ("Pass", &["Result", "ResultPath", "Parameters"], &[]),
    (
        "Task",
        &[
            "Resource",
            "Parameters",
            "ResultSelector",
            "ResultPath",
            "Retry",
            "Catch",
            "TimeoutSeconds",
            "TimeoutSecondsPath",
            "HeartbeatSeconds",
            "HeartbeatSecondsPath",
            "Credentials",
        ],
        &[
            "Resource",
            "Arguments",
            "Retry",
            "Catch",
            "TimeoutSeconds",
            "HeartbeatSeconds",
            "Credentials",
        ],
    ),
    ("Choice", &["Choices", "Default"], &["Choices", "Default"]),
    (
        "Wait",
        &["Seconds", "SecondsPath", "Timestamp", "TimestampPath"],
        &["Seconds", "Timestamp"],
    ),
    (
        "Fail",
        &["Error", "ErrorPath", "Cause", "CausePath"],
        &["Error", "Cause"],
    ),
    (
        "Parallel",
        &[
            "Branches",
            "Parameters",
            "ResultSelector",
            "ResultPath",
            "Retry",
            "Catch",
        ],
        &["Branches", "Arguments", "Retry", "Catch"],
    ),
    (
        "Map",
        &[
            "ItemProcessor",
            "Iterator",
            "ItemsPath",
            "ItemSelector",
            "Parameters",
            "MaxConcurrency",
            "MaxConcurrencyPath",
            "ItemReader",
            "ItemBatcher",
            "ResultWriter",
            "ResultSelector",
            "ResultPath",
            "Retry",
            "Catch",
            "ToleratedFailureCount",
            "ToleratedFailureCountPath",
            "ToleratedFailurePercentage",
            "ToleratedFailurePercentagePath",
            "Label",
        ],
        &[
            "ItemProcessor",
            "Items",
            "ItemSelector",
            "MaxConcurrency",
            "ItemReader",
            "ItemBatcher",
            "ResultWriter",
            "Retry",
            "Catch",
            "ToleratedFailureCount",
            "ToleratedFailurePercentage",
            "Label",
            "Arguments",
        ],
    ),
];

/// The fields every state of a language may carry, then each type's own.
fn allowed(kind: &str, lang: Lang) -> Vec<&'static str> {
    let mut all = vec!["Type", "Comment", "QueryLanguage", "Assign"];
    if !matches!(kind, "Choice" | "Succeed" | "Fail") {
        all.extend(["Next", "End"]);
    }
    if kind != "Fail" {
        match lang {
            Lang::JsonPath => all.extend(["InputPath", "OutputPath"]),
            Lang::Jsonata => all.push("Output"),
        }
    }
    if let Some((_, path, ata)) = FIELDS.iter().find(|(k, ..)| *k == kind) {
        all.extend(match lang {
            Lang::JsonPath => *path,
            Lang::Jsonata => *ata,
        });
    }
    all
}

pub(super) fn state(
    name: &str,
    body: &Value,
    inherited: Lang,
    at: &str,
    depth: usize,
    out: &mut Vec<Diagnostic>,
) -> Option<State> {
    let Value::Object(map) = body else {
        out.push(Diagnostic::error(
            SCHEMA,
            "The value for the field 'State' must be an OBJECT",
            at,
        ));
        return None;
    };
    let obj = Obj::new(map, at.to_owned());
    let lang = language(&obj, inherited, out);
    let ty = obj.text("Type", out);
    let known = ty.as_deref().is_some_and(|t| {
        [
            "Task", "Wait", "Pass", "Succeed", "Fail", "Choice", "Parallel", "Map",
        ]
        .contains(&t)
    });
    if ty.is_none() {
        obj.require(&["Type"], out);
    } else if !known {
        out.push(Diagnostic::error(
            SCHEMA,
            format!("The field 'Type' should have one of these values: {TYPES}"),
            &obj.here("Type"),
        ));
    }
    let mut s = State {
        name: name.to_owned(),
        lang,
        kind: Kind::Pass { result: None },
        next: None,
        end: false,
        input_path: Field::Absent,
        output_path: Field::Absent,
        result_path: Field::Absent,
        parameters: None,
        result_selector: None,
        arguments: None,
        output: None,
        assign: None,
        retry: Vec::new(),
        catch: Vec::new(),
        unknown: true,
    };
    let Some(ty) = ty.filter(|_| known) else {
        // A state whose type is unknown is kept as a placeholder, so that the transitions to it are not reported as well.
        return Some(s);
    };
    s.unknown = false;
    obj.only(&allowed(&ty, lang), out);
    s.next = obj.text("Next", out);
    s.end = obj.flag("End", out);
    s.input_path = obj.path("InputPath", out);
    s.output_path = obj.path("OutputPath", out);
    s.result_path = obj.path("ResultPath", out);
    s.parameters = obj.get("Parameters").cloned();
    s.result_selector = obj.get("ResultSelector").cloned();
    s.arguments = obj.get("Arguments").cloned();
    s.output = obj.get("Output").cloned();
    s.assign = obj.get("Assign").cloned();
    s.retry = retriers(&obj, out);
    s.catch = catchers(&obj, out);
    s.kind = kind(&ty, &obj, lang, depth, out);
    Some(s)
}

fn kind(ty: &str, obj: &Obj<'_>, lang: Lang, depth: usize, out: &mut Vec<Diagnostic>) -> Kind {
    match ty {
        "Pass" => Kind::Pass {
            result: obj.get("Result").cloned(),
        },
        "Task" => {
            obj.require(&["Resource"], out);
            Kind::Task(Box::new(Task {
                resource: obj.text("Resource", out).unwrap_or_default(),
                timeout: obj.num("TimeoutSeconds", out),
                heartbeat: obj.num("HeartbeatSeconds", out),
                credentials: obj.get("Credentials").cloned(),
            }))
        }
        "Choice" => super::choice::choice(obj, lang, out),
        "Wait" => Kind::Wait(wait(obj, out)),
        "Succeed" => Kind::Succeed,
        "Fail" => Kind::Fail(Fail {
            error: obj.get("Error").cloned(),
            error_path: obj.text("ErrorPath", out),
            cause: obj.get("Cause").cloned(),
            cause_path: obj.text("CausePath", out),
        }),
        "Parallel" => {
            obj.require(&["Branches"], out);
            let mut branches = Vec::new();
            for (i, b) in obj.array("Branches", out).into_iter().flatten().enumerate() {
                let at = format!("{}/Branches/{i}", obj.at);
                if let Value::Object(m) = b {
                    let sub = Obj::new(m, at);
                    sub.only(&["StartAt", "States", "Comment"], out);
                    branches.extend(machine(&sub, lang, depth + 1, out));
                }
            }
            Kind::Parallel { branches }
        }
        _ => map(obj, lang, depth, out),
    }
}

fn wait(obj: &Obj<'_>, out: &mut Vec<Diagnostic>) -> Wait {
    let given = ["Seconds", "SecondsPath", "Timestamp", "TimestampPath"]
        .iter()
        .filter(|f| obj.get(f).is_some())
        .count();
    if given != 1 {
        // (memory)
        out.push(Diagnostic::error(
            SCHEMA,
            "Exactly one of these fields is required: [Seconds, SecondsPath, Timestamp, TimestampPath]",
            &obj.at,
        ));
    }
    match (obj.get("Timestamp"), obj.get("TimestampPath")) {
        (Some(v @ Value::String(s)), _) if crate::jsonata::is_expr(s) => {
            Wait::Timestamp(TimeSource::Expr(v.clone()))
        }
        (Some(Value::String(s)), _) => Wait::Timestamp(TimeSource::Fixed(s.clone())),
        (_, Some(Value::String(p))) => Wait::Timestamp(TimeSource::Path(p.clone())),
        _ => Wait::Seconds(obj.num("Seconds", out)),
    }
}

fn map(obj: &Obj<'_>, lang: Lang, depth: usize, out: &mut Vec<Diagnostic>) -> Kind {
    let (key, proc) = match (
        obj.object("ItemProcessor", out),
        obj.object("Iterator", out),
    ) {
        (Some(p), _) => ("ItemProcessor", Some(p)),
        (None, p) => ("Iterator", p),
    };
    if proc.is_none() {
        obj.require(&["ItemProcessor"], out);
    }
    let sub = proc.map(|m| Obj::new(m, obj.here(key)));
    let config = sub.as_ref().and_then(|s| s.object("ProcessorConfig", out));
    let mode = config.and_then(|c| c.get("Mode")).and_then(Value::as_str);
    if let Some(s) = &sub {
        s.only(&["StartAt", "States", "Comment", "ProcessorConfig"], out);
    }
    let processor = sub
        .as_ref()
        .and_then(|s| machine(s, lang, depth + 1, out))
        .unwrap_or(super::Definition {
            start_at: String::new(),
            states: Vec::new(),
            timeout_seconds: None,
            lang,
        });
    Kind::Map(Box::new(MapSpec {
        processor,
        distributed: mode == Some("DISTRIBUTED"),
        execution_type: config
            .and_then(|c| c.get("ExecutionType"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        items_path: obj.path("ItemsPath", out),
        items: obj.get("Items").cloned(),
        item_selector: obj
            .get("ItemSelector")
            .or_else(|| obj.get("Parameters"))
            .cloned(),
        max_concurrency: obj.num("MaxConcurrency", out),
        item_reader: obj.get("ItemReader").cloned(),
        item_batcher: obj.get("ItemBatcher").cloned(),
        result_writer: obj.get("ResultWriter").cloned(),
        tolerated_count: obj.num("ToleratedFailureCount", out),
        tolerated_percentage: obj.num("ToleratedFailurePercentage", out),
        label: obj.text("Label", out),
    }))
}

fn errors_of(o: &Obj<'_>, out: &mut Vec<Diagnostic>) -> Vec<String> {
    o.require(&["ErrorEquals"], out);
    o.array("ErrorEquals", out)
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str().map(str::to_owned))
        .collect()
}

fn retriers(obj: &Obj<'_>, out: &mut Vec<Diagnostic>) -> Vec<Retrier> {
    let mut all = Vec::new();
    for (i, r) in obj.array("Retry", out).into_iter().flatten().enumerate() {
        let Value::Object(m) = r else { continue };
        let o = Obj::new(m, format!("{}/Retry/{i}", obj.at));
        o.only(
            &[
                "ErrorEquals",
                "IntervalSeconds",
                "MaxAttempts",
                "BackoffRate",
                "MaxDelaySeconds",
                "JitterStrategy",
                "Comment",
            ],
            out,
        );
        all.push(Retrier {
            errors: errors_of(&o, out),
            interval: o.number("IntervalSeconds", out).unwrap_or(1.0),
            max_attempts: o
                .number("MaxAttempts", out)
                .map_or(3, |n| if n < 0.0 { 0 } else { n as u32 }),
            backoff: o.number("BackoffRate", out).unwrap_or(2.0),
            max_delay: o.number("MaxDelaySeconds", out),
            jitter: o.text("JitterStrategy", out).as_deref() == Some("FULL"),
        });
    }
    all
}

fn catchers(obj: &Obj<'_>, out: &mut Vec<Diagnostic>) -> Vec<Catcher> {
    let mut all = Vec::new();
    for (i, c) in obj.array("Catch", out).into_iter().flatten().enumerate() {
        let Value::Object(m) = c else { continue };
        let o = Obj::new(m, format!("{}/Catch/{i}", obj.at));
        o.only(
            &[
                "ErrorEquals",
                "Next",
                "ResultPath",
                "Assign",
                "Output",
                "Comment",
            ],
            out,
        );
        o.require(&["Next"], out);
        all.push(Catcher {
            errors: errors_of(&o, out),
            next: o.text("Next", out).unwrap_or_default(),
            result_path: o.path("ResultPath", out),
            assign: o.get("Assign").cloned(),
            output: o.get("Output").cloned(),
        });
    }
    all
}
