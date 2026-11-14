//! cty's `format` and `formatlist` (`stdlib/format.go`, `format_fsm.rl`): printf-like verbs read by
//! a small state machine, each formatting one argument after converting it to what the verb needs.

use super::encoding::marshal;
use super::print;
use super::{Spec, list, str_arg};
use crate::convert::convert;
use crate::function::{FuncError, Param};
use crate::types::Type;
use crate::value::{Refinement, Value};

pub(super) fn functions() -> Vec<(&'static str, Spec)> {
    let args = Param::new("args", Type::Dynamic).null().unknown().dynamic();
    let fmt = || vec![Param::new("format", Type::String)];
    vec![
        (
            "format",
            Spec::new(fmt(), Type::String, format).var(args.clone()),
        ),
        (
            "formatlist",
            Spec::new(fmt(), Type::list(Type::String), formatlist).var(args),
        ),
    ]
}

/// One verb: `%`, flags, width, precision, argument index, mode letter.
#[derive(Debug, Clone, Default)]
pub(super) struct Verb {
    pub(super) raw: String,
    pub(super) offset: usize,
    pub(super) arg: usize,
    pub(super) mode: char,
    pub(super) zero: bool,
    pub(super) sharp: bool,
    pub(super) plus: bool,
    pub(super) minus: bool,
    pub(super) space: bool,
    pub(super) prec: Option<usize>,
    pub(super) width: Option<usize>,
}

/// Widths and precisions past this are refused (Go would try to write them; its own `fmt` gives up
/// on numbers past a million).
const MAX_WIDTH: usize = 1_000_000;

type Out = Result<Value, FuncError>;

fn format(args: &[Value], _: &Type) -> Out {
    let f = str_arg(args, 0);
    let rest = args.get(1..).unwrap_or_default();
    if !rest.iter().all(Value::is_wholly_known) {
        return Ok(match f.find('%') {
            Some(i) if i > 0 => Value::refined(
                Type::String,
                Refinement {
                    prefix: crate::value::refine::safe_prefix(f.get(..i).unwrap_or("")),
                    ..Refinement::default()
                },
            ),
            _ => Value::unknown(Type::String),
        });
    }
    format_fsm(f, rest).map(|s| Value::string(&s))
}

/// The arguments of `formatlist` and the length of their sequences.
type Shape = (Vec<Each>, Option<usize>);

/// What a `formatlist` argument contributes to each line.
enum Each {
    Seq(Vec<Value>),
    One(Value),
}

fn formatlist(args: &[Value], _: &Type) -> Out {
    let rest = args.get(1..).unwrap_or_default();
    if rest.is_empty() {
        let line = format(args, &Type::String)?;
        return list(Type::String, vec![line]);
    }
    let Some((each, len)) = classify(rest)? else {
        return Ok(Value::unknown(Type::list(Type::String)));
    };
    lines(str_arg(args, 0), &each, len.unwrap_or(1))
}

/// Each argument as a sequence (one element per line) or a single value (on every line), and the
/// sequences' common length; `None` while an argument's shape is not yet known.
fn classify(rest: &[Value]) -> Result<Option<Shape>, FuncError> {
    let mut each = Vec::with_capacity(rest.len());
    let mut len: Option<(usize, usize)> = None;
    let mut unknown = false;
    for (i, a) in rest.iter().enumerate() {
        let t = a.ty();
        let seq = matches!(t, Type::List(_) | Type::Set(_) | Type::Tuple(_)) && !a.is_null();
        if !seq {
            unknown |= *a == Value::DYNAMIC;
            each.push(Each::One(a.clone()));
            continue;
        }
        let n = match (a, &t) {
            (_, Type::Tuple(ts)) => ts.len(),
            (Value::List(_, es) | Value::Set(_, es), _) if !set_unknown_len(a) => es.len(),
            _ => {
                unknown = true;
                continue;
            }
        };
        match len {
            None => len = Some((n, i)),
            Some((l, at)) if l != n => {
                return Err(FuncError::arg(
                    i + 1,
                    format!(
                        "argument {} has length {n}, which is inconsistent with argument {} of length {l}",
                        i + 1,
                        at + 1
                    ),
                ));
            }
            Some(_) => {}
        }
        unknown |= !a.is_known();
        each.push(Each::Seq(
            a.elements().into_iter().map(|(_, v)| v).collect(),
        ));
    }
    Ok((!unknown).then(|| (each, len.map(|(n, _)| n))))
}

/// One formatted line per element of the sequences.
fn lines(f: &str, each: &[Each], n: usize) -> Out {
    let mut lines = Vec::with_capacity(n);
    for i in 0..n {
        let line: Vec<Value> = each
            .iter()
            .map(|e| match e {
                Each::Seq(vs) => vs.get(i).cloned().unwrap_or(Value::DYNAMIC),
                Each::One(v) => v.clone(),
            })
            .collect();
        if !line.iter().all(Value::is_wholly_known) {
            lines.push(Value::unknown_not_null(Type::String));
            continue;
        }
        let s = format_fsm(f, &line)
            .map_err(|e| FuncError::new(format!("error on format iteration {i}: {}", e.msg)))?;
        lines.push(Value::string(&s));
    }
    if lines.is_empty() {
        return Ok(Value::List(Type::String, Vec::new()));
    }
    list(Type::String, lines)
}

/// Whether a set's length is not yet known: it holds unknowns that may turn out equal to others.
fn set_unknown_len(v: &Value) -> bool {
    match v {
        Value::Set(_, es) => es.len() > 1 && !v.is_wholly_known(),
        _ => false,
    }
}

/// The scanner states between verbs.
fn format_fsm(f: &str, args: &[Value]) -> Result<String, FuncError> {
    let mut out = String::new();
    let mut next_arg = 1;
    let mut highest = 0;
    let b = f.as_bytes();
    let mut p = 0;
    while let Some(&c) = b.get(p) {
        if c != b'%' {
            let ch = f
                .get(p..)
                .and_then(|r| r.chars().next())
                .unwrap_or('\u{fffd}');
            out.push(ch);
            p += ch.len_utf8();
            continue;
        }
        if b.get(p + 1) == Some(&b'%') {
            out.push('%');
            p += 2;
            continue;
        }
        let verb = parse_verb(f, p, next_arg)?;
        p += verb.raw.len();
        highest = highest.max(verb.arg);
        append(&verb, &mut out, args)?;
        next_arg = verb.arg + 1;
    }
    if highest < args.len() {
        let msg = if highest == 0 {
            "too many arguments; no verbs in format string".to_owned()
        } else {
            format!("too many arguments; only {highest} used by format string")
        };
        return Err(FuncError::arg(highest + 1, msg));
    }
    Ok(out)
}

/// Reads one verb starting at the `%` at `start`.
fn parse_verb(f: &str, start: usize, next_arg: usize) -> Result<Verb, FuncError> {
    let b = f.as_bytes();
    let mut v = Verb {
        offset: start,
        arg: next_arg,
        ..Verb::default()
    };
    let mut p = start + 1;
    while let Some(c) = b.get(p) {
        match c {
            b'0' => v.zero = true,
            b'#' => v.sharp = true,
            b'-' => v.minus = true,
            b'+' => v.plus = true,
            b' ' => v.space = true,
            _ => break,
        }
        p += 1;
    }
    if b.get(p).is_some_and(|c| (b'1'..=b'9').contains(c)) {
        v.width = Some(digits(f, &mut p)?);
    }
    if b.get(p) == Some(&b'.') {
        p += 1;
        v.prec = Some(digits(f, &mut p)?);
    }
    if b.get(p) == Some(&b'[') {
        p += 1;
        if !b.get(p).is_some_and(|c| (b'1'..=b'9').contains(c)) {
            return Err(bad_char(f, p));
        }
        v.arg = digits(f, &mut p)?;
        if b.get(p) != Some(&b']') {
            return Err(bad_char(f, p));
        }
        p += 1;
    }
    match b.get(p) {
        Some(c) if c.is_ascii_alphabetic() => v.mode = char::from(*c),
        _ => return Err(bad_char(f, p)),
    }
    v.raw = f.get(start..=p).unwrap_or("").to_owned();
    Ok(v)
}

/// Decimal digits at `p` (any number of them, possibly none).
fn digits(f: &str, p: &mut usize) -> Result<usize, FuncError> {
    let b = f.as_bytes();
    let mut n: usize = 0;
    while let Some(c) = b.get(*p).filter(|c| c.is_ascii_digit()) {
        n = n.saturating_mul(10).saturating_add(usize::from(c - b'0'));
        *p += 1;
    }
    if n > MAX_WIDTH {
        return Err(FuncError::new(format!(
            "width or precision {n} is larger than {MAX_WIDTH}"
        )));
    }
    Ok(n)
}

fn bad_char(f: &str, p: usize) -> FuncError {
    match f.get(p..).and_then(|r| r.chars().next()) {
        Some(c) => FuncError::new(format!(
            "unrecognized format character {} at offset {p}",
            go_rune_quote(c)
        )),
        None => FuncError::new(format!("invalid format string starting at offset {p}")),
    }
}

/// Go's `%q` of a rune.
fn go_rune_quote(c: char) -> String {
    let q = crate::value::quote::go_quote(c.encode_utf8(&mut [0; 4]));
    let inner = q.get(1..q.len().saturating_sub(1)).unwrap_or("");
    let inner = if inner == "\\\"" { "\"" } else { inner };
    let inner = if inner == "'" { "\\'" } else { inner };
    format!("'{inner}'")
}

/// `formatAppend`: one verb's text.
fn append(v: &Verb, out: &mut String, args: &[Value]) -> Result<(), FuncError> {
    let Some(a) = v.arg.checked_sub(1).and_then(|i| args.get(i)) else {
        return Err(FuncError::new(format!(
            "not enough arguments for {} at {}: need index {} but have {} total",
            crate::value::quote::go_quote(&v.raw),
            v.offset,
            v.arg,
            args.len()
        )));
    };
    if v.mode != 'v' && a.is_null() {
        return Err(unsupported(v, "null value cannot be formatted"));
    }
    let text = match v.mode {
        'v' => as_is(v, a)?,
        't' => match convert(a, &Type::Bool).map_err(|e| unsupported(v, &e.0))? {
            Value::Bool(true) => "true".to_owned(),
            _ => "false".to_owned(),
        },
        'b' | 'd' | 'o' | 'x' | 'X' | 'e' | 'E' | 'f' | 'g' | 'G' => number(v, a)?,
        's' | 'q' => string(v, a)?,
        m => {
            return Err(FuncError::new(format!(
                "unsupported format verb {} in {} at offset {}",
                go_rune_quote(m),
                crate::value::quote::go_quote(&v.raw),
                v.offset
            )));
        }
    };
    out.push_str(&text);
    Ok(())
}

fn unsupported(v: &Verb, why: &str) -> FuncError {
    FuncError::new(format!(
        "unsupported value for {} at {}: {why}",
        crate::value::quote::go_quote(&v.raw),
        v.offset
    ))
}

/// `%v`: strings as they are, numbers in shortest `%g` form, anything else (and everything with
/// `#`) as JSON.
fn as_is(v: &Verb, a: &Value) -> Result<String, FuncError> {
    match a {
        Value::String(s) if !v.sharp => Ok(print::pad(v, s)),
        Value::Number(n) if !v.sharp => Ok(print::pad(v, &n.text_with(b'g', -1))),
        _ => {
            let json = marshal(a).map_err(|e| unsupported(v, &e))?;
            Ok(print::pad(v, &json))
        }
    }
}

fn number(v: &Verb, a: &Value) -> Result<String, FuncError> {
    let n = convert(a, &Type::Number).map_err(|e| unsupported(v, &e.0))?;
    let Some(n) = n.as_number() else {
        return Err(unsupported(v, "a number is required"));
    };
    if matches!(v.mode, 'b' | 'd' | 'o' | 'x' | 'X') {
        let (neg, mag) = match n.trunc() {
            Some((neg, mag)) if n.is_integer() => (neg, mag),
            _ => return Err(unsupported(v, "an integer is required")),
        };
        return Ok(print::int(
            v,
            neg && mag != num_bigint::BigUint::default(),
            &mag,
        ));
    }
    Ok(print::float(v, n))
}

fn string(v: &Verb, a: &Value) -> Result<String, FuncError> {
    let s = convert(a, &Type::String).map_err(|e| unsupported(v, &e.0))?;
    let mut s = s.as_str().unwrap_or("").to_owned();
    if let Some(p) = v.prec.filter(|p| *p > 0) {
        let end = super::graphemes::clusters(&s)
            .get(p)
            .map_or(s.len(), |(i, _)| *i);
        s.truncate(end);
    }
    if v.mode == 'q' {
        let mut q = String::new();
        print::json_string(&mut q, &s);
        return Ok(print::pad(v, &q));
    }
    Ok(print::pad(v, &s))
}
