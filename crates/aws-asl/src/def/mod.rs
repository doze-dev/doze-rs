//! A state machine definition, read from its JSON once and checked as AWS checks it. The checks report every problem AWS would, in
//! AWS's form (`CODE: message at /pointer`), so a definition that AWS refuses is refused here with the same words where they were
//! recorded, and one AWS takes is taken.

mod check;
mod choice;
mod fields;
mod parse;
mod state;

pub use choice::{CmpKind, CmpOp, Operand, Rule, Test};
pub use parse::{parse, parse_value};

use serde_json::Value;

/// The query language of a machine or one state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    JsonPath,
    Jsonata,
}

/// A path field, which may be absent (its default applies), `null` (it discards), or a path.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Field {
    #[default]
    Absent,
    Null,
    Text(String),
}

/// A number a state reads: written in the definition, read from the input at a path, or a JSONata expression.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Num {
    #[default]
    None,
    Fixed(f64),
    Path(String),
    Expr(Value),
}

/// A (sub-)machine: the root, a Parallel branch, or a Map's item processor.
#[derive(Clone, Debug)]
pub struct Definition {
    pub start_at: String,
    /// In document order.
    pub states: Vec<State>,
    pub timeout_seconds: Option<f64>,
    pub lang: Lang,
}

impl Definition {
    /// The state named `name`.
    #[must_use]
    pub fn state(&self, name: &str) -> Option<&State> {
        self.states.iter().find(|s| s.name == name)
    }
}

/// One state.
#[derive(Clone, Debug)]
pub struct State {
    pub name: String,
    pub lang: Lang,
    pub kind: Kind,
    pub next: Option<String>,
    pub end: bool,
    pub input_path: Field,
    pub output_path: Field,
    pub result_path: Field,
    pub parameters: Option<Value>,
    pub result_selector: Option<Value>,
    pub arguments: Option<Value>,
    pub output: Option<Value>,
    pub assign: Option<Value>,
    pub retry: Vec<Retrier>,
    pub catch: Vec<Catcher>,
    /// A state whose `Type` was missing or unknown, kept so that transitions to it are not reported too.
    pub unknown: bool,
}

impl State {
    /// The type's name as history spells it (`Pass`, `Task`, ...).
    #[must_use]
    pub const fn type_name(&self) -> &'static str {
        match self.kind {
            Kind::Pass { .. } => "Pass",
            Kind::Task(_) => "Task",
            Kind::Choice { .. } => "Choice",
            Kind::Wait(_) => "Wait",
            Kind::Succeed => "Succeed",
            Kind::Fail(_) => "Fail",
            Kind::Parallel { .. } => "Parallel",
            Kind::Map(_) => "Map",
        }
    }

    /// Whether the state ends its (sub-)machine when it completes.
    #[must_use]
    pub const fn terminal(&self) -> bool {
        self.end || matches!(self.kind, Kind::Succeed | Kind::Fail(_))
    }
}

/// What a state of each type carries.
#[derive(Clone, Debug)]
pub enum Kind {
    Pass {
        result: Option<Value>,
    },
    Task(Box<Task>),
    Choice {
        rules: Vec<Rule>,
        default: Option<String>,
    },
    Wait(Wait),
    Succeed,
    Fail(Fail),
    Parallel {
        branches: Vec<Definition>,
    },
    Map(Box<MapSpec>),
}

/// A Task state.
#[derive(Clone, Debug)]
pub struct Task {
    pub resource: String,
    pub timeout: Num,
    pub heartbeat: Num,
    pub credentials: Option<Value>,
}

/// How long a Wait state waits.
#[derive(Clone, Debug)]
pub enum Wait {
    Seconds(Num),
    Timestamp(TimeSource),
}

/// Where a Wait state's timestamp comes from.
#[derive(Clone, Debug)]
pub enum TimeSource {
    Fixed(String),
    Path(String),
    Expr(Value),
}

/// A Fail state: each of the error and the cause is fixed, read at a path, or a JSONata expression.
#[derive(Clone, Debug, Default)]
pub struct Fail {
    pub error: Option<Value>,
    pub error_path: Option<String>,
    pub cause: Option<Value>,
    pub cause_path: Option<String>,
}

/// A Map state.
#[derive(Clone, Debug)]
pub struct MapSpec {
    pub processor: Definition,
    /// `ProcessorConfig.Mode` is `DISTRIBUTED`.
    pub distributed: bool,
    /// `ProcessorConfig.ExecutionType` of a distributed map (`STANDARD` or `EXPRESS`).
    pub execution_type: Option<String>,
    pub items_path: Field,
    pub items: Option<Value>,
    pub item_selector: Option<Value>,
    pub max_concurrency: Num,
    pub item_reader: Option<Value>,
    pub item_batcher: Option<Value>,
    pub result_writer: Option<Value>,
    pub tolerated_count: Num,
    pub tolerated_percentage: Num,
    pub label: Option<String>,
}

/// One entry of `Retry`.
#[derive(Clone, Debug)]
pub struct Retrier {
    pub errors: Vec<String>,
    pub interval: f64,
    pub max_attempts: u32,
    pub backoff: f64,
    pub max_delay: Option<f64>,
    pub jitter: bool,
}

/// One entry of `Catch`.
#[derive(Clone, Debug)]
pub struct Catcher {
    pub errors: Vec<String>,
    pub next: String,
    pub result_path: Field,
    pub assign: Option<Value>,
    pub output: Option<Value>,
}

/// One problem with a definition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    /// `SCHEMA_VALIDATION_FAILED`, `MISSING_TRANSITION_TARGET`, `MISSING_END_STATE`, `INVALID_JSON_DESCRIPTION`, ...
    pub code: &'static str,
    pub message: String,
    /// A JSON pointer (`/States/A/Type`); `None` for the machine as a whole (AWS prints `null`).
    pub location: Option<String>,
    /// `ERROR` or `WARNING`.
    pub warning: bool,
}

impl Diagnostic {
    pub(crate) fn error(code: &'static str, message: impl Into<String>, at: &str) -> Self {
        Self {
            code,
            message: message.into(),
            location: Some(if at.is_empty() {
                "/".to_owned()
            } else {
                at.to_owned()
            }),
            warning: false,
        }
    }

    /// `CODE: message at location`, as `InvalidDefinition` lists them.
    #[must_use]
    pub fn line(&self) -> String {
        format!(
            "{}: {} at {}",
            self.code,
            self.message,
            self.location.as_deref().unwrap_or("null")
        )
    }
}

/// The message of `InvalidDefinition` for a list of problems.
#[must_use]
pub fn refusal(diagnostics: &[Diagnostic]) -> String {
    let lines: Vec<String> = diagnostics
        .iter()
        .filter(|d| !d.warning)
        .map(Diagnostic::line)
        .collect();
    format!("Invalid State Machine Definition: '{}'", lines.join(", "))
}

/// Visits every Task resource in a definition and its branches.
pub fn resources<'a>(def: &'a Definition, out: &mut Vec<&'a str>) {
    for s in &def.states {
        match &s.kind {
            Kind::Task(t) => out.push(&t.resource),
            Kind::Parallel { branches } => branches.iter().for_each(|b| resources(b, out)),
            Kind::Map(m) => resources(&m.processor, out),
            _ => {}
        }
    }
}
