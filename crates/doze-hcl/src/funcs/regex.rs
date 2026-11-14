//! cty's `regex` (`stdlib/regexp.go`): the first match of a Go regular expression, as a string (no
//! capture groups), a tuple (unnamed groups) or an object (named groups); groups that took no part
//! in the match are null strings.

use super::regex_syntax::translate;
use super::{Spec, arg, str_arg};
use crate::function::{FuncError, Param};
use crate::types::Type;
use crate::value::Value;
use regex::{Regex, RegexBuilder};
use std::collections::BTreeMap;

pub(super) fn functions() -> Vec<(&'static str, Spec)> {
    vec![(
        "regex",
        Spec::rule(
            vec![
                Param::new("pattern", Type::String),
                Param::new("string", Type::String),
            ],
            regex_type,
            regex,
        ),
    )]
}

/// The crate's nesting limit, above what the translator's 250 levels of groups can need (a group
/// may be wrapped for a repetition and hold a class).
const NEST_LIMIT: u32 = 1100;

/// The crate's compiled-size limit. Go counts a class as one instruction, the crate compiles it to
/// UTF-8 automata, so a large Unicode class repeated many times (`\pL{0,1000}`) needs room: 64 MiB
/// takes it (in about a quarter of a second); larger patterns are refused (catalogue F20).
const SIZE_LIMIT: usize = 64 << 20;

/// The pattern compiled, with each group's name.
fn compile(pattern: &str) -> Result<(Regex, Vec<Option<String>>), String> {
    let t = translate(pattern)?;
    let re = RegexBuilder::new(&t.pattern)
        .nest_limit(NEST_LIMIT)
        .size_limit(SIZE_LIMIT)
        .build()
        .map_err(|e| format!("error parsing pattern: {e}"))?;
    Ok((re, t.names))
}

/// `regexPatternResultType`: what the groups make of the result.
fn result_type(names: &[Option<String>]) -> Result<Type, String> {
    let named: Vec<&String> = names.iter().flatten().collect();
    let unnamed = names.len() - named.len();
    match (unnamed, named.len()) {
        (0, 0) => Ok(Type::String),
        (u, 0) => Ok(Type::Tuple(vec![Type::String; u])),
        (0, _) => Ok(Type::object(
            named
                .into_iter()
                .map(|n| (n.clone(), Type::String))
                .collect(),
        )),
        _ => Err(
            "invalid regexp pattern: cannot mix both named and unnamed capture groups".to_owned(),
        ),
    }
}

fn regex_type(args: &[Value]) -> Result<Type, FuncError> {
    if !arg(args, 0).is_known() {
        return Ok(Type::Dynamic);
    }
    // Go's rules decide validity and the groups; compiling waits for the call.
    let t = translate(str_arg(args, 0)).map_err(|e| FuncError::arg(0, e))?;
    result_type(&t.names).map_err(|e| FuncError::arg(0, e))
}

fn regex(args: &[Value], ret: &Type) -> Result<Value, FuncError> {
    if *ret == Type::Dynamic {
        return Ok(Value::DYNAMIC);
    }
    let (re, names) = compile(str_arg(args, 0)).map_err(|e| FuncError::arg(0, e))?;
    let s = str_arg(args, 1);
    let Some(caps) = re.captures(s) else {
        return Err(FuncError::new(
            "pattern did not match any part of the given string",
        ));
    };
    let group = |i: usize| {
        caps.get(i)
            .map_or(Value::Null(Type::String), |m| Value::string(m.as_str()))
    };
    Ok(match ret {
        Type::String => group(0),
        Type::Tuple(_) => Value::Tuple((1..=names.len()).map(group).collect()),
        _ => {
            let mut attrs = BTreeMap::new();
            for (i, name) in names.iter().enumerate() {
                if let Some(n) = name {
                    attrs.insert(n.clone(), group(i + 1));
                }
            }
            Value::Object(attrs)
        }
    })
}
