//! cty's string functions (`stdlib/string.go`, `string_replace.go`).

use super::case;
use super::graphemes;
use super::{Spec, arg, at_least, list, str_arg, to_int};
use crate::function::{FuncError, Param};
use crate::types::Type;
use crate::value::Value;

pub(super) fn functions() -> Vec<(&'static str, Spec)> {
    let s = |name| Param::new(name, Type::String);
    let one = |imp| Spec::new(vec![s("str")], Type::String, imp);
    let two = |second, imp| Spec::new(vec![s("str"), s(second)], Type::String, imp);
    vec![
        (
            "upper",
            Spec::new(vec![s("str").dynamic()], Type::String, upper),
        ),
        (
            "lower",
            Spec::new(vec![s("str").dynamic()], Type::String, lower),
        ),
        ("title", one(title)),
        ("trimspace", one(trimspace)),
        ("chomp", one(chomp)),
        ("trim", two("cutset", trim)),
        ("trimprefix", two("prefix", trimprefix)),
        ("trimsuffix", two("suffix", trimsuffix)),
        (
            "replace",
            Spec::new(
                vec![s("str"), s("substr"), s("replace")],
                Type::String,
                replace,
            ),
        ),
        (
            "split",
            Spec::new(
                vec![s("separator"), s("str")],
                Type::list(Type::String),
                split,
            ),
        ),
        (
            "join",
            Spec::new(vec![s("separator")], Type::String, join)
                .var(Param::new("lists", Type::list(Type::String))),
        ),
        (
            "strlen",
            Spec::new(vec![s("str").unknown().dynamic()], Type::Number, strlen)
                .refine(Some(at_least(0))),
        ),
        (
            "substr",
            Spec::new(
                vec![
                    s("str").dynamic(),
                    Param::new("offset", Type::Number).dynamic(),
                    Param::new("length", Type::Number).dynamic(),
                ],
                Type::String,
                substr,
            ),
        ),
        (
            "indent",
            Spec::new(
                vec![Param::new("spaces", Type::Number), s("str")],
                Type::String,
                indent,
            ),
        ),
    ]
}

type Out = Result<Value, FuncError>;

fn upper(args: &[Value], _: &Type) -> Out {
    Ok(Value::string(
        &str_arg(args, 0)
            .chars()
            .map(case::upper)
            .collect::<String>(),
    ))
}

fn lower(args: &[Value], _: &Type) -> Out {
    Ok(Value::string(
        &str_arg(args, 0)
            .chars()
            .map(case::lower)
            .collect::<String>(),
    ))
}

fn title(args: &[Value], _: &Type) -> Out {
    Ok(Value::string(&case::title_words(str_arg(args, 0))))
}

fn trimspace(args: &[Value], _: &Type) -> Out {
    Ok(Value::string(str_arg(args, 0).trim()))
}

/// `(?:\r\n?|\n)*\z` removed: every trailing `\r` and `\n`.
fn chomp(args: &[Value], _: &Type) -> Out {
    Ok(Value::string(
        str_arg(args, 0).trim_end_matches(['\r', '\n']),
    ))
}

fn trim(args: &[Value], _: &Type) -> Out {
    let cutset = str_arg(args, 1);
    Ok(Value::string(
        str_arg(args, 0).trim_matches(|c| cutset.contains(c)),
    ))
}

fn trimprefix(args: &[Value], _: &Type) -> Out {
    let s = str_arg(args, 0);
    Ok(Value::string(s.strip_prefix(str_arg(args, 1)).unwrap_or(s)))
}

fn trimsuffix(args: &[Value], _: &Type) -> Out {
    let s = str_arg(args, 0);
    Ok(Value::string(s.strip_suffix(str_arg(args, 1)).unwrap_or(s)))
}

/// `strings.Replace(s, old, new, -1)`: an empty `old` matches before every character and at the end.
fn replace(args: &[Value], _: &Type) -> Out {
    let (s, old, new) = (str_arg(args, 0), str_arg(args, 1), str_arg(args, 2));
    Ok(Value::string(&s.replace(old, new)))
}

/// `strings.Split`: an empty separator splits into characters (none for an empty string).
fn split(args: &[Value], _: &Type) -> Out {
    let (sep, s) = (str_arg(args, 0), str_arg(args, 1));
    let parts: Vec<Value> = if sep.is_empty() {
        s.chars()
            .map(|c| Value::string(c.encode_utf8(&mut [0; 4])))
            .collect()
    } else {
        s.split(sep).map(Value::string).collect()
    };
    list(Type::String, parts)
}

fn join(args: &[Value], _: &Type) -> Out {
    let sep = str_arg(args, 0);
    let lists = args.get(1..).unwrap_or_default();
    if lists.is_empty() {
        return Err(FuncError::new("at least one list is required"));
    }
    if !lists.iter().all(Value::is_wholly_known) {
        return Ok(Value::unknown(Type::String));
    }
    let mut items = Vec::new();
    for (ai, l) in lists.iter().enumerate() {
        for (ei, (_, v)) in l.elements().into_iter().enumerate() {
            let Some(s) = v.as_str() else {
                let msg = if lists.len() > 1 {
                    format!(
                        "element {ei} of list {} is null; cannot concatenate null values",
                        ai + 1
                    )
                } else {
                    format!("element {ei} is null; cannot concatenate null values")
                };
                return Err(FuncError::arg(ai + 1, msg));
            };
            items.push(s.to_owned());
        }
    }
    Ok(Value::string(&items.join(sep)))
}

/// The grapheme clusters; for an unknown string, at least those of its known prefix.
fn strlen(args: &[Value], _: &Type) -> Out {
    let v = arg(args, 0);
    if let Value::Unknown(t, r) = v {
        let prefix = r.as_ref().map_or("", |r| r.prefix.as_str());
        if *t == Type::String {
            return Ok(Value::unknown(Type::Number).refine_with(&at_least(
                i64::try_from(graphemes::count(prefix)).unwrap_or(i64::MAX),
            )));
        }
        return Ok(Value::unknown(Type::Number));
    }
    let n = i64::try_from(graphemes::count(str_arg(args, 0))).unwrap_or(i64::MAX);
    Ok(Value::int(n))
}

/// cty's `substr`, quirks included: a negative offset counts from the end (and past the start is the
/// start); a length of zero after a negative offset takes the rest.
fn substr(args: &[Value], _: &Type) -> Out {
    let s = str_arg(args, 0);
    let mut offset = to_int(arg(args, 1)).map_err(FuncError::new)?;
    let length = to_int(arg(args, 2)).map_err(FuncError::new)?;
    let clusters = graphemes::clusters(s);
    let total = i64::try_from(clusters.len()).unwrap_or(i64::MAX);
    if offset < 0 {
        offset = offset.saturating_add(total);
    } else if length == 0 {
        return Ok(Value::string(""));
    }
    let start = if offset > 0 {
        if offset > total {
            return Ok(Value::string(""));
        }
        usize::try_from(offset).unwrap_or(usize::MAX)
    } else {
        0
    };
    let rest = clusters.get(start..).unwrap_or_default();
    let take = match usize::try_from(length) {
        Ok(n) if n > 0 => n.min(rest.len()),
        // Go's loop never reaches a count of zero, and a negative length means "the rest".
        _ => rest.len(),
    };
    let from = rest.first().map_or(s.len(), |c| c.0);
    let to = rest.get(take).map_or(s.len(), |c| c.0);
    Ok(Value::string(s.get(from..to).unwrap_or("")))
}

/// The most `indent` writes; Go would try to allocate whatever it is asked for.
const INDENT_LIMIT: u128 = 1 << 26;

fn indent(args: &[Value], _: &Type) -> Out {
    let spaces = to_int(arg(args, 0)).map_err(FuncError::new)?;
    let data = str_arg(args, 1);
    let Ok(n) = usize::try_from(spaces) else {
        return Err(super::panic_error("strings: negative Repeat count"));
    };
    let lines = data.matches('\n').count();
    if lines > 0 && (n as u128) * (lines as u128) > INDENT_LIMIT {
        return Err(FuncError::new(format!(
            "indenting by {n} spaces would make a string longer than {INDENT_LIMIT} bytes"
        )));
    }
    let pad = format!("\n{}", " ".repeat(if lines > 0 { n } else { 0 }));
    Ok(Value::string(&data.replace('\n', &pad)))
}
