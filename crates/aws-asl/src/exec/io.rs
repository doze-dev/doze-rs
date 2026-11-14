//! How a state's input becomes its output: in JSONPath, `InputPath`, `Parameters` (or `ItemSelector`), then the result through
//! `ResultSelector`, `ResultPath` (into the *raw* input) and `OutputPath`; in JSONata, `Arguments` and `Output` over `$states`.
//! The context object (`$$`) and the variables (`$name`) are read here too.

use serde_json::{Map, Value, json};

use super::{Exec, Frame};
use crate::def::{Field, Lang, State};
use crate::env::Env;
use crate::errors::{
    Failure, PARAMETER_PATH_FAILURE, QUERY_EVALUATION_ERROR, RESULT_PATH_MATCH_FAILURE, RUNTIME,
};
use crate::path::{Path, Scope, set_at};

/// How the context object writes a time.
#[must_use]
pub fn context_time(ms: i64) -> String {
    crate::time::format(ms)
}

/// What a state is read with.
pub(crate) struct View<'a> {
    pub(crate) ex: &'a Exec,
    pub(crate) frame: &'a Frame,
    pub(crate) state: &'a State,
    pub(crate) env: &'a Env,
}

pub(crate) fn parse(text: &str) -> Value {
    serde_json::from_str(text).unwrap_or(Value::Null)
}

pub(crate) fn text(v: &Value) -> String {
    v.to_string()
}

impl View<'_> {
    /// `$$`, with `Task.Token` when there is one.
    pub(crate) fn context(&self, token: Option<&str>) -> Value {
        let f = self.frame;
        let retries: u32 = f.attempts.iter().sum();
        let mut c = json!({
            "Execution": {
                "Id": self.ex.arn, "Input": parse(&self.ex.input), "Name": self.ex.name,
                "RoleArn": self.ex.role_arn, "StartTime": context_time(self.ex.start_ms),
                "RedriveCount": self.ex.redrive_count,
            },
            "State": {"EnteredTime": context_time(f.entered_ms), "Name": f.state, "RetryCount": retries},
            "StateMachine": {"Id": self.ex.machine_arn, "Name": self.ex.machine_name},
        });
        if let Some(m) = c.as_object_mut() {
            if let Some(t) = token.or(f.token.as_deref()) {
                m.insert("Task".into(), json!({"Token": t}));
            }
            if let Some(item) = &f.item {
                m.insert(
                    "Map".into(),
                    json!({"Item": {"Index": f.index, "Value": parse(item)}}),
                );
            }
        }
        c
    }

    pub(crate) fn vars(&self) -> Value {
        parse(&self.frame.vars)
    }

    /// A States.Runtime-style failure, worded as AWS words the start of it (recorded).
    pub(crate) fn fail(&self, error: &str, what: &str) -> Failure {
        Failure::new(
            error,
            format!(
                "An error occurred while executing the state '{}' (entered at the event id #{}). {what}",
                self.state.name, self.frame.entered_event
            ),
        )
    }

    /// `InputPath` applied to the raw input.
    pub(crate) fn effective(&self, raw: &Value) -> Result<Value, Failure> {
        self.select_path(&self.state.input_path, raw, "InputPath")
    }

    /// `OutputPath` applied to what the state made.
    pub(crate) fn output(&self, made: &Value) -> Result<Value, Failure> {
        self.select_path(&self.state.output_path, made, "OutputPath")
    }

    fn select_path(&self, f: &Field, doc: &Value, _field: &str) -> Result<Value, Failure> {
        match f {
            Field::Absent => Ok(doc.clone()),
            Field::Null => Ok(json!({})),
            Field::Text(p) => {
                let path = Path::parse(p).map_err(|e| self.fail(RUNTIME, &e))?;
                let (ctx, vars) = (self.context(None), self.vars());
                path.select(Scope {
                    data: doc,
                    context: &ctx,
                    vars: &vars,
                })
                .ok_or_else(|| {
                    // (memory)
                    self.fail(
                        RUNTIME,
                        &format!(
                            "Invalid path '{p}' : No results for path: {}",
                            bracketed(&path)
                        ),
                    )
                })
            }
        }
    }

    /// A payload template (`Parameters`, `ResultSelector`, `ItemSelector`, a JSONPath `Assign`): every `.$` member is evaluated.
    pub(crate) fn template(
        &self,
        t: &Value,
        data: &Value,
        token: Option<&str>,
    ) -> Result<Value, Failure> {
        let (ctx, vars) = (self.context(token), self.vars());
        let scope = Scope {
            data,
            context: &ctx,
            vars: &vars,
        };
        self.fill(t, scope)
    }

    fn fill(&self, t: &Value, scope: Scope<'_>) -> Result<Value, Failure> {
        match t {
            Value::Object(m) => {
                let mut out = Map::new();
                for (k, v) in m {
                    if let (Some(name), Some(expr)) = (k.strip_suffix(".$"), v.as_str()) {
                        out.insert(name.to_owned(), self.expr(k, expr, scope)?);
                    } else {
                        out.insert(k.clone(), self.fill(v, scope)?);
                    }
                }
                Ok(Value::Object(out))
            }
            Value::Array(a) => a
                .iter()
                .map(|v| self.fill(v, scope))
                .collect::<Result<_, _>>()
                .map(Value::Array),
            other => Ok(other.clone()),
        }
    }

    fn expr(&self, field: &str, expr: &str, scope: Scope<'_>) -> Result<Value, Failure> {
        if expr.starts_with("States.") {
            return crate::intrinsic::eval(expr, scope, self.env)
                .map_err(|f| self.fail(&f.error, &f.cause));
        }
        let path = Path::parse(expr).map_err(|e| self.fail(PARAMETER_PATH_FAILURE, &e))?;
        path.select(scope).ok_or_else(|| {
            self.fail(
                RUNTIME,
                &format!(
                    "The JSONPath '{expr}' specified for the field '{field}' could not be found in the input '{}'",
                    scope.data
                ),
            )
        })
    }

    /// `ResultPath`: `result` written into the raw input (absent: the result replaces it; `null`: the input is kept).
    pub(crate) fn merge(&self, path: &Field, raw: &Value, result: Value) -> Result<Value, Failure> {
        match path {
            Field::Absent => Ok(result),
            Field::Null => Ok(raw.clone()),
            Field::Text(p) => {
                let parsed =
                    Path::parse(p).map_err(|e| self.fail(RESULT_PATH_MATCH_FAILURE, &e))?;
                let mut doc = raw.clone();
                set_at(&mut doc, &parsed, result).map_err(|_| {
                    // (memory)
                    self.fail(
                        RESULT_PATH_MATCH_FAILURE,
                        &format!("Unable to apply ResultPath \"{p}\" to input \"{raw}\""),
                    )
                })?;
                Ok(doc)
            }
        }
    }

    /// `$states` for a JSONata expression.
    pub(crate) fn states(&self, input: &Value, extra: &[(&str, Value)]) -> Value {
        let mut m = Map::new();
        m.insert("input".into(), input.clone());
        m.insert("context".into(), self.context(None));
        for (k, v) in extra {
            m.insert((*k).to_owned(), v.clone());
        }
        Value::Object(m)
    }

    /// Evaluates a JSONata template for `field` (`Arguments`, `Output`, ...). `None` when the whole template is undefined.
    pub(crate) fn jsonata(
        &self,
        field: &str,
        t: &Value,
        states: &Value,
    ) -> Result<Option<Value>, Failure> {
        let vars = match self.vars() {
            Value::Object(m) => m,
            _ => Map::new(),
        };
        crate::jsonata::eval(t, crate::jsonata::Scope { states, vars: &vars }, self.env).map_err(|e| {
            // (memory)
            self.fail(
                QUERY_EVALUATION_ERROR,
                &format!(
                    "The JSONata expression '{}' specified for the field '{field}' returned an error: {}",
                    e.expr, e.why
                ),
            )
        })
    }

    /// The variables an `Assign` sets, evaluated against `data` (JSONPath) or `$states` (JSONata).
    pub(crate) fn assign(
        &self,
        assign: Option<&Value>,
        data: &Value,
        states: &Value,
    ) -> Result<Option<Map<String, Value>>, Failure> {
        let Some(a) = assign else { return Ok(None) };
        let made = if self.state.lang == Lang::Jsonata {
            self.jsonata("Assign", a, states)?.unwrap_or(Value::Null)
        } else {
            self.template(a, data, None)?
        };
        Ok(match made {
            Value::Object(m) => Some(m),
            _ => None,
        })
    }
}

/// Writes assigned variables into a frame's scope.
pub(crate) fn set_vars(f: &mut Frame, set: &Map<String, Value>) {
    let mut vars = match parse(&f.vars) {
        Value::Object(m) => m,
        _ => Map::new(),
    };
    for (k, v) in set {
        vars.insert(k.clone(), v.clone());
    }
    f.vars = Value::Object(vars).to_string();
}

/// `$['a']['b']`, as Jayway prints a path in its messages.
fn bracketed(p: &Path) -> String {
    let mut s = String::from("$");
    for seg in &p.segs {
        match seg {
            crate::path::Seg::Field(n) => s.push_str(&format!("['{n}']")),
            crate::path::Seg::Index(i) => s.push_str(&format!("[{i}]")),
            _ => s.push_str("[*]"),
        }
    }
    s
}
