//! A command's arguments that read the same in an app, a service and a task (Go's
//! `internal/runspec`): the readiness probes and their functions, steps, sleep, ready_timeout and
//! console.

use crate::gotime;
use crate::model::{DEFAULT_READY_TIMEOUT, Probe, Sleep};
use doze_hcl::Value;
use doze_hcl::function::{FuncError, Function, Functions, Param};
use doze_hcl::types::{ObjectType, Type};
use std::collections::BTreeMap;
use std::time::Duration;

fn probe_type() -> Type {
    Type::Object(ObjectType::new(
        [("arg", Type::String), ("kind", Type::String)]
            .into_iter()
            .map(|(k, t)| (k.to_owned(), t))
            .collect(),
    ))
}

/// `tcp()`, `http(path)`, `log(text)`, `exec(command)`: a probe as an object.
struct ProbeFunc {
    kind: &'static str,
    params: Vec<Param>,
}

impl Function for ProbeFunc {
    fn params(&self) -> &[Param] {
        &self.params
    }
    fn return_type(&self, _args: &[Value]) -> Result<Type, FuncError> {
        Ok(probe_type())
    }
    fn call(&self, args: &[Value], _ret: &Type) -> Result<Value, FuncError> {
        let arg = args
            .first()
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        Ok(Value::Object(BTreeMap::from([
            ("arg".to_owned(), Value::string(&arg)),
            ("kind".to_owned(), Value::string(self.kind)),
        ])))
    }
}

/// The probe functions a command body sees besides doze.hcl's own (Go's `runspec.Functions`).
pub fn functions() -> Functions {
    let mut fs = Functions::new();
    let probes: [(&'static str, Option<&'static str>); 4] = [
        ("tcp", None),
        ("http", Some("path")),
        ("log", Some("text")),
        ("exec", Some("command")),
    ];
    for (kind, param) in probes {
        let params = param
            .map(|p| vec![Param::new(p, Type::String)])
            .unwrap_or_default();
        fs.insert(kind.to_owned(), Box::new(ProbeFunc { kind, params }));
    }
    fs
}

/// Go's `ParseReady`: a probe object, or none.
pub fn parse_ready(v: &Value) -> Result<Option<Probe>, String> {
    if v.is_null() {
        return Ok(None);
    }
    let bad =
        || "ready takes a probe: tcp(), http(\"/path\"), log(\"text\") or exec(\"cmd\")".to_owned();
    if !v.is_known() || v.ty() != probe_type() {
        return Err(bad());
    }
    let Value::Object(m) = v else {
        return Err(bad());
    };
    let get = |k: &str| {
        m.get(k)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    let p = Probe {
        kind: get("kind"),
        arg: get("arg"),
    };
    match p.kind.as_str() {
        "http" if !p.arg.starts_with('/') => Err(format!(
            "ready = http({}): the path must start with \"/\" (the host and port are the block's own)",
            doze_hcl::value::quote::go_quote(&p.arg)
        )),
        "log" | "exec" if p.arg.trim().is_empty() => {
            Err(format!("ready = {}(…) needs a non-empty argument", p.kind))
        }
        _ => Ok(Some(p)),
    }
}

/// Go's `Steps`: a command or a list of commands, none empty.
pub fn steps(attr: &str, v: &Value) -> Result<Vec<String>, String> {
    if v.is_null() {
        return Ok(Vec::new());
    }
    let bad = || format!("{attr} takes a command or a list of commands");
    let out = match v {
        Value::String(s) => vec![s.clone()],
        Value::List(_, es) | Value::Set(_, es) | Value::Tuple(es) => es
            .iter()
            .map(|e| match e {
                Value::String(s) => Ok(s.clone()),
                _ => Err(bad()),
            })
            .collect::<Result<_, _>>()?,
        _ => return Err(bad()),
    };
    if out.iter().any(|s| s.trim().is_empty()) {
        return Err(format!("{attr} has an empty command"));
    }
    Ok(out)
}

/// Go's `ParseSleep`: "", "never", or a positive duration.
pub fn parse_sleep(s: &str) -> Result<Sleep, String> {
    match s {
        "" => Ok(Sleep::default()),
        "never" => Ok(Sleep {
            set: true,
            never: true,
            after: Duration::ZERO,
        }),
        _ => match gotime::parse(s) {
            Some(n) if n > 0 => Ok(Sleep {
                set: true,
                never: false,
                after: Duration::from_nanos(n.unsigned_abs()),
            }),
            _ => Err(format!(
                "sleep = {}: want a duration such as \"10m\", or \"never\"",
                doze_hcl::value::quote::go_quote(s)
            )),
        },
    }
}

/// Go's `ParseReadyTimeout`: the default for "", else a positive duration.
pub fn parse_ready_timeout(s: &str) -> Result<Duration, String> {
    if s.is_empty() {
        return Ok(DEFAULT_READY_TIMEOUT);
    }
    match gotime::parse(s) {
        Some(n) if n > 0 => Ok(Duration::from_nanos(n.unsigned_abs())),
        _ => Err(format!(
            "ready_timeout = {}: want a duration such as \"90s\"",
            doze_hcl::value::quote::go_quote(s)
        )),
    }
}

/// Go's `CheckConsole`: an http(s) URL or nothing.
pub fn check_console(s: &str) -> Result<(), String> {
    if s.is_empty() || s.starts_with("http://") || s.starts_with("https://") {
        return Ok(());
    }
    Err(format!(
        "console = {}: want an http:// or https:// URL",
        doze_hcl::value::quote::go_quote(s)
    ))
}
