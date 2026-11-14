//! The function library. Wording of the refusals is from memory; the results of the functions the recording exercises are checked
//! against it (`test_the_states_language::intrinsics`).

use md5::Md5;
use serde_json::{Map, Value};
use sha1::Sha1;
use sha2::{Digest, Sha256, Sha384, Sha512};

use super::Call;
use crate::env::Env;

const NAMES: &[(&str, usize, usize)] = &[
    ("States.Format", 1, usize::MAX),
    ("States.StringToJson", 1, 1),
    ("States.JsonToString", 1, 1),
    ("States.Array", 0, usize::MAX),
    ("States.ArrayPartition", 2, 2),
    ("States.ArrayContains", 2, 2),
    ("States.ArrayRange", 3, 3),
    ("States.ArrayGetItem", 2, 2),
    ("States.ArrayLength", 1, 1),
    ("States.ArrayUnique", 1, 1),
    ("States.Base64Encode", 1, 1),
    ("States.Base64Decode", 1, 1),
    ("States.Hash", 2, 2),
    ("States.JsonMerge", 3, 3),
    ("States.MathRandom", 2, 3),
    ("States.MathAdd", 2, 2),
    ("States.StringSplit", 2, 2),
    ("States.UUID", 0, 0),
];

/// Whether every call in `call` names a function that exists, with an allowed number of arguments.
pub(super) fn known(call: &Call) -> bool {
    NAMES
        .iter()
        .any(|(n, lo, hi)| *n == call.name && (*lo..=*hi).contains(&call.args.len()))
        && call.args.iter().all(|a| match a {
            super::Arg::Call(c) => known(c),
            _ => true,
        })
}

pub(super) fn apply(name: &str, a: &[Value], env: &Env) -> Result<Value, String> {
    let (_, lo, hi) = NAMES
        .iter()
        .find(|(n, ..)| *n == name)
        .ok_or_else(|| format!("The function {name} is not supported"))?;
    if !(*lo..=*hi).contains(&a.len()) {
        return Err(format!(
            "The function {name} was given {} arguments",
            a.len()
        ));
    }
    match name {
        "States.Format" => format(a),
        "States.StringToJson" => serde_json::from_str(text(a, 0)?)
            .map_err(|_| "The argument of States.StringToJson is not JSON".to_owned()),
        "States.JsonToString" => Ok(Value::String(arg(a, 0).to_string())),
        "States.Array" => Ok(Value::Array(a.to_vec())),
        "States.ArrayPartition" => partition(list(a, 0)?, int(a, 1)?),
        "States.ArrayContains" => Ok(Value::Bool(list(a, 0)?.contains(arg(a, 1)))),
        "States.ArrayRange" => range(int(a, 0)?, int(a, 1)?, int(a, 2)?),
        "States.ArrayGetItem" => get_item(list(a, 0)?, int(a, 1)?),
        "States.ArrayLength" => Ok(Value::from(list(a, 0)?.len())),
        "States.ArrayUnique" => Ok(unique(list(a, 0)?)),
        "States.Base64Encode" => Ok(Value::String(aws_core::base64::encode(
            text(a, 0)?.as_bytes(),
        ))),
        "States.Base64Decode" => aws_core::base64::decode(text(a, 0)?)
            .and_then(|b| String::from_utf8(b).ok())
            .map(Value::String)
            .ok_or_else(|| "The argument of States.Base64Decode is not base64 text".to_owned()),
        "States.Hash" => hash(arg(a, 0), text(a, 1)?),
        "States.JsonMerge" => merge(a),
        "States.MathRandom" => random(int(a, 0)?, int(a, 1)?, env),
        "States.MathAdd" => int(a, 0)?
            .checked_add(int(a, 1)?)
            .map(Value::from)
            .ok_or_else(|| "States.MathAdd overflowed".to_owned()),
        "States.StringSplit" => Ok(split(text(a, 0)?, text(a, 1)?)),
        _ => Ok(Value::String(env.uuid())),
    }
}

fn arg(a: &[Value], i: usize) -> &Value {
    a.get(i).unwrap_or(&Value::Null)
}

fn text(a: &[Value], i: usize) -> Result<&str, String> {
    arg(a, i)
        .as_str()
        .ok_or_else(|| format!("Argument {} must be a string", i + 1))
}

fn list(a: &[Value], i: usize) -> Result<&Vec<Value>, String> {
    arg(a, i)
        .as_array()
        .ok_or_else(|| format!("Argument {} must be an array", i + 1))
}

fn int(a: &[Value], i: usize) -> Result<i64, String> {
    let v = arg(a, i);
    v.as_i64()
        .or_else(|| v.as_f64().filter(|f| f.fract() == 0.0).map(|f| f as i64))
        .ok_or_else(|| format!("Argument {} must be an integer", i + 1))
}

/// `{}` is replaced by the next argument (a string as it is, anything else as JSON); `\{` and `\}` are braces.
fn format(a: &[Value]) -> Result<Value, String> {
    let template = text(a, 0)?;
    let mut rest = a.iter().skip(1);
    let mut out = String::new();
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => out.extend(chars.next()),
            '{' if chars.peek() == Some(&'}') => {
                chars.next();
                let v = rest.next().ok_or_else(|| {
                    "The number of arguments does not match the number of placeholders".to_owned()
                })?;
                match v {
                    Value::String(s) => out.push_str(s),
                    other => out.push_str(&other.to_string()),
                }
            }
            c => out.push(c),
        }
    }
    if rest.next().is_some() {
        return Err("The number of arguments does not match the number of placeholders".to_owned());
    }
    Ok(Value::String(out))
}

fn partition(items: &[Value], size: i64) -> Result<Value, String> {
    let size = usize::try_from(size)
        .ok()
        .filter(|s| *s > 0)
        .ok_or_else(|| "The chunk size must be a positive integer".to_owned())?;
    Ok(Value::Array(
        items
            .chunks(size)
            .map(|c| Value::Array(c.to_vec()))
            .collect(),
    ))
}

fn range(start: i64, end: i64, step: i64) -> Result<Value, String> {
    if step == 0 {
        return Err("The step of States.ArrayRange cannot be 0".to_owned());
    }
    let mut out = Vec::new();
    let mut i = start;
    while (step > 0 && i <= end) || (step < 0 && i >= end) {
        out.push(Value::from(i));
        if out.len() > 1000 {
            return Err("States.ArrayRange cannot make more than 1000 items".to_owned());
        }
        i += step;
    }
    Ok(Value::Array(out))
}

fn get_item(items: &[Value], index: i64) -> Result<Value, String> {
    usize::try_from(index)
        .ok()
        .and_then(|i| items.get(i))
        .cloned()
        .ok_or_else(|| format!("Index {index} is out of the bounds of the array"))
}

fn unique(items: &[Value]) -> Value {
    let mut out: Vec<Value> = Vec::new();
    for v in items {
        if !out.contains(v) {
            out.push(v.clone());
        }
    }
    Value::Array(out)
}

fn hash(data: &Value, algorithm: &str) -> Result<Value, String> {
    let data = match data {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    let bytes = data.as_bytes();
    let digest = match algorithm {
        "MD5" => Md5::digest(bytes).to_vec(),
        "SHA-1" => Sha1::digest(bytes).to_vec(),
        "SHA-256" => Sha256::digest(bytes).to_vec(),
        "SHA-384" => Sha384::digest(bytes).to_vec(),
        "SHA-512" => Sha512::digest(bytes).to_vec(),
        other => {
            return Err(format!(
                "The hash algorithm {other} is not supported: use one of MD5, SHA-1, SHA-256, SHA-384, SHA-512"
            ));
        }
    };
    Ok(Value::String(aws_core::hex::encode(&digest)))
}

fn merge(a: &[Value]) -> Result<Value, String> {
    let (Value::Object(left), Value::Object(right)) = (arg(a, 0), arg(a, 1)) else {
        return Err("The first two arguments of States.JsonMerge must be objects".to_owned());
    };
    if arg(a, 2).as_bool() != Some(false) {
        return Err(
            "States.JsonMerge supports only a shallow merge: its third argument must be false"
                .to_owned(),
        );
    }
    let mut out: Map<String, Value> = left.clone();
    for (k, v) in right {
        out.insert(k.clone(), v.clone());
    }
    Ok(Value::Object(out))
}

fn random(start: i64, end: i64, env: &Env) -> Result<Value, String> {
    if end <= start {
        return Err("The end of States.MathRandom must be greater than its start".to_owned());
    }
    let span = u64::try_from(end - start).unwrap_or(1).max(1);
    let pick = i64::try_from(env.next_u64() % span).unwrap_or(0);
    Ok(Value::from(start + pick))
}

/// Every character of `delimiters` separates; empty pieces are dropped (Java's `StringTokenizer`, from memory).
fn split(text: &str, delimiters: &str) -> Value {
    Value::Array(
        text.split(|c| delimiters.contains(c))
            .filter(|s| !s.is_empty())
            .map(|s| Value::String(s.to_owned()))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::env::Env;
    use crate::path::Scope;

    fn run(text: &str, data: serde_json::Value) -> serde_json::Value {
        let none = json!({});
        super::super::eval(
            text,
            Scope {
                data: &data,
                context: &none,
                vars: &none,
            },
            &Env::new(1, 7),
        )
        .unwrap()
    }

    #[test]
    fn the_recorded_results() {
        let d = json!({"a": "x", "b": "y", "n": 37, "csv": "p,q,r", "obj": {"k": 1}, "other": {"j": 2},
            "text": "{\"z\": [1, 2]}", "list": [1, 2, 3], "dupes": [1, 1, 2]});
        assert_eq!(
            run("States.Format('{}-{}', $.a, $.b)", d.clone()),
            json!("x-y")
        );
        assert_eq!(
            run("States.Array($.a, $.b, 3)", d.clone()),
            json!(["x", "y", 3])
        );
        assert_eq!(run("States.MathAdd($.n, 5)", d.clone()), json!(42));
        assert_eq!(
            run("States.StringSplit($.csv, ',')", d.clone()),
            json!(["p", "q", "r"])
        );
        assert_eq!(
            run("States.JsonToString($.obj)", d.clone()),
            json!("{\"k\":1}")
        );
        assert_eq!(
            run("States.StringToJson($.text)", d.clone()),
            json!({"z": [1, 2]})
        );
        assert_eq!(
            run("States.ArrayContains($.list, 2)", d.clone()),
            json!(true)
        );
        assert_eq!(
            run("States.ArrayRange(1, 9, 3)", d.clone()),
            json!([1, 4, 7])
        );
        assert_eq!(
            run("States.ArrayPartition($.list, 2)", d.clone()),
            json!([[1, 2], [3]])
        );
        assert_eq!(run("States.ArrayUnique($.dupes)", d.clone()), json!([1, 2]));
        assert_eq!(run("States.ArrayGetItem($.list, 1)", d.clone()), json!(2));
        assert_eq!(run("States.Base64Encode($.a)", d.clone()), json!("eA=="));
        assert_eq!(
            run("States.Hash($.a, 'SHA-256')", d.clone()),
            json!("2d711642b726b04401627ca9fbac32f5c8530fb1903cc4db02258717921a4881")
        );
        assert_eq!(
            run("States.JsonMerge($.obj, $.other, false)", d),
            json!({"k": 1, "j": 2})
        );
    }

    #[test]
    fn the_others() {
        assert_eq!(run("States.Base64Decode('eA==')", json!({})), json!("x"));
        assert_eq!(
            run("States.ArrayLength(States.Array(1, 2))", json!({})),
            json!(2)
        );
        let r = run("States.MathRandom(1, 10)", json!({}));
        assert!((1..10).contains(&r.as_i64().unwrap()));
        assert_eq!(run("States.UUID()", json!({})).as_str().unwrap().len(), 36);
        assert_eq!(
            run(r"States.Format('\{{}\}', 'x')", json!({})),
            json!("{x}")
        );
    }
}
