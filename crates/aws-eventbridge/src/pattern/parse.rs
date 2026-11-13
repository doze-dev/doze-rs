//! Reading a pattern. The words of a refusal are AWS's `Event pattern is not valid. Reason: <reason>` and then the place, as Jackson
//! prints it. Recorded (2026-11-09): text that is not JSON, and a field whose value is neither an object nor an array. Every other
//! reason is from memory.

use std::net::IpAddr;

use super::json::{self, Json, Spanned, line_and_column};
use super::{Cmp, Cond, Node, Object, Pattern, Text};

/// Why a pattern was refused, and the byte AWS points at.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Refusal {
    reason: String,
    at: usize,
}

impl Refusal {
    fn new(reason: impl Into<String>, at: &Spanned) -> Self {
        // Jackson has read the first character of the value when it complains, so it points one past where the value began.
        Self {
            reason: reason.into(),
            at: at.at + 1,
        }
    }

    /// The message of `InvalidEventPatternException` for `text`.
    pub(crate) fn message(&self, text: &str) -> String {
        let (line, column) = line_and_column(text, self.at);
        format!(
            "Event pattern is not valid. Reason: {}\n at [Source: (String)\"{text}\"; line: {line}, column: {column}]",
            self.reason
        )
    }

    /// Just the reason.
    #[cfg(test)]
    pub(crate) fn reason(&self) -> &str {
        &self.reason
    }
}

// fuzz:parser fuzz_event_pattern
/// Reads `text` as an event pattern.
///
/// # Errors
/// What is wrong with it.
pub(crate) fn parse(text: &str) -> Result<Pattern, Refusal> {
    let root = json::read(text).map_err(|s| Refusal {
        reason: s.message,
        at: s.at,
    })?;
    let Json::Obj(fields) = &root.v else {
        return Err(Refusal::new("Event pattern must be a JSON object", &root));
    };
    object(fields, 0).map(Pattern)
}

/// How deep patterns may nest.
const MAX_DEPTH: usize = 32;

fn object(fields: &[(String, Spanned)], depth: usize) -> Result<Object, Refusal> {
    let mut out = Object::default();
    for (key, value) in fields {
        if key == "$or" {
            out.any_of.push(alternatives(value, depth)?);
            continue;
        }
        let node = match &value.v {
            Json::Obj(inner) if depth < MAX_DEPTH => Node::Nested(object(inner, depth + 1)?),
            Json::Obj(_) => return Err(Refusal::new("Pattern nests too deeply", value)),
            Json::Arr(items) => Node::Leaf(conditions(value, items)?),
            _ => {
                return Err(Refusal::new(
                    format!("\"{key}\" must be an object or an array"),
                    value,
                ));
            }
        };
        out.keys.push((key.clone(), node));
    }
    Ok(out)
}

fn alternatives(value: &Spanned, depth: usize) -> Result<Vec<Object>, Refusal> {
    let Json::Arr(items) = &value.v else {
        return Err(Refusal::new("$or value must be an array", value));
    };
    if items.is_empty() {
        return Err(Refusal::new("Empty arrays are not allowed", value));
    }
    items
        .iter()
        .map(|item| match &item.v {
            Json::Obj(inner) if depth < MAX_DEPTH => object(inner, depth + 1),
            _ => Err(Refusal::new("$or must contain only objects", item)),
        })
        .collect()
}

fn conditions(array: &Spanned, items: &[Spanned]) -> Result<Vec<Cond>, Refusal> {
    if items.is_empty() {
        return Err(Refusal::new("Empty arrays are not allowed", array));
    }
    items.iter().map(condition).collect()
}

fn condition(item: &Spanned) -> Result<Cond, Refusal> {
    match &item.v {
        Json::Null => Ok(Cond::Null),
        Json::Bool(b) => Ok(Cond::Bool(*b)),
        Json::Num(n) => Ok(Cond::Num(*n)),
        Json::Str(s) => Ok(Cond::Text(Text::Exact(s.clone()))),
        Json::Obj(fields) => operator(fields, item),
        Json::Arr(_) => Err(Refusal::new(
            "Match value must be String, number, true, false, or null",
            item,
        )),
    }
}

fn operator(fields: &[(String, Spanned)], whole: &Spanned) -> Result<Cond, Refusal> {
    let [(name, arg)] = fields else {
        return Err(Refusal::new(
            "Match value must be String, number, true, false, or null",
            whole,
        ));
    };
    match name.as_str() {
        "prefix" => affix(arg, Text::Prefix, Text::PrefixIgnoreCase),
        "suffix" => affix(arg, Text::Suffix, Text::SuffixIgnoreCase),
        "equals-ignore-case" => {
            string(arg, "equals-ignore-case").map(|s| text(Text::EqualsIgnoreCase(s)))
        }
        "wildcard" => wildcard(arg),
        "anything-but" => anything_but(arg),
        "numeric" => numeric(arg),
        "exists" => match arg.v {
            Json::Bool(b) => Ok(Cond::Exists(b)),
            _ => Err(Refusal::new(
                "exists match pattern must be either true or false.",
                arg,
            )),
        },
        "cidr" => cidr(arg),
        other => Err(Refusal::new(
            format!("Unrecognized match type {other}"),
            whole,
        )),
    }
}

const fn text(t: Text) -> Cond {
    Cond::Text(t)
}

fn string(arg: &Spanned, operator: &str) -> Result<String, Refusal> {
    match &arg.v {
        Json::Str(s) => Ok(s.clone()),
        _ => Err(Refusal::new(
            format!("{operator} match pattern must be a string"),
            arg,
        )),
    }
}

/// `{"prefix": "a"}` or `{"prefix": {"equals-ignore-case": "a"}}`.
fn affix(
    arg: &Spanned,
    plain: fn(String) -> Text,
    ignoring_case: fn(String) -> Text,
) -> Result<Cond, Refusal> {
    match &arg.v {
        Json::Str(s) => Ok(text(plain(s.clone()))),
        Json::Obj(f) => match f.as_slice() {
            [(k, v)] if k == "equals-ignore-case" => {
                string(v, "equals-ignore-case").map(|s| text(ignoring_case(s)))
            }
            _ => Err(Refusal::new("Unsupported matcher configuration", arg)),
        },
        _ => Err(Refusal::new(
            "prefix/suffix match pattern must be a string",
            arg,
        )),
    }
}

fn wildcard(arg: &Spanned) -> Result<Cond, Refusal> {
    let s = string(arg, "wildcard")?;
    super::wildcard::check(&s).map_err(|why| Refusal::new(why, arg))?;
    Ok(text(Text::Wildcard(s)))
}

fn cidr(arg: &Spanned) -> Result<Cond, Refusal> {
    let s = string(arg, "cidr")?;
    let bad = || Refusal::new("Malformed CIDR, one '/' required", arg);
    let (addr, bits) = s.split_once('/').ok_or_else(bad)?;
    let addr: IpAddr = addr
        .parse()
        .map_err(|_| Refusal::new("Nonstandard IP address", arg))?;
    let bits: u8 = bits
        .parse()
        .map_err(|_| Refusal::new("Malformed CIDR, mask bits must be an integer", arg))?;
    let limit = if addr.is_ipv4() { 32 } else { 128 };
    if bits > limit {
        return Err(Refusal::new("Malformed CIDR, mask bits too large", arg));
    }
    Ok(Cond::Cidr(addr, bits))
}

fn anything_but(arg: &Spanned) -> Result<Cond, Refusal> {
    let one = |v: &Spanned| match &v.v {
        Json::Str(s) => Ok(text(Text::Exact(s.clone()))),
        Json::Num(n) => Ok(Cond::Num(*n)),
        _ => Err(Refusal::new(
            "Inside anything-but list, start|null|boolean is not supported.",
            v,
        )),
    };
    match &arg.v {
        Json::Str(_) | Json::Num(_) => Ok(Cond::AnythingBut(vec![one(arg)?])),
        Json::Arr(items) if !items.is_empty() => Ok(Cond::AnythingBut(
            items.iter().map(one).collect::<Result<_, _>>()?,
        )),
        Json::Obj(f) => anything_but_object(f, arg),
        _ => Err(Refusal::new(
            "anything-but only supports Strings, Numbers and lists of them",
            arg,
        )),
    }
}

/// `{"anything-but": {"prefix": "a"}}` and the other text operators, with one string or a list of them.
fn anything_but_object(fields: &[(String, Spanned)], whole: &Spanned) -> Result<Cond, Refusal> {
    let [(name, arg)] = fields else {
        return Err(Refusal::new("Unsupported matcher configuration", whole));
    };
    let make: fn(String) -> Text = match name.as_str() {
        "prefix" => Text::Prefix,
        "suffix" => Text::Suffix,
        "equals-ignore-case" => Text::EqualsIgnoreCase,
        "wildcard" => Text::Wildcard,
        other => {
            return Err(Refusal::new(
                format!("Unrecognized match type {other}"),
                whole,
            ));
        }
    };
    let items: Vec<&Spanned> = match &arg.v {
        Json::Arr(items) => items.iter().collect(),
        _ => vec![arg],
    };
    let mut out = Vec::new();
    for item in items {
        let s = string(item, name)?;
        if name == "wildcard" {
            super::wildcard::check(&s).map_err(|why| Refusal::new(why, item))?;
        }
        out.push(text(make(s)));
    }
    Ok(Cond::AnythingBut(out))
}

fn numeric(arg: &Spanned) -> Result<Cond, Refusal> {
    let Json::Arr(items) = &arg.v else {
        return Err(Refusal::new("Value of numeric must be an array.", arg));
    };
    let bad = || Refusal::new("Bad numeric range operator", arg);
    let mut pairs = Vec::new();
    for pair in items.chunks(2) {
        let [op, n] = pair else {
            return Err(Refusal::new(
                "Value of numeric must be an array of operator and number pairs",
                arg,
            ));
        };
        let (Json::Str(op), Json::Num(n)) = (&op.v, &n.v) else {
            return Err(bad());
        };
        let cmp = match op.as_str() {
            "=" => Cmp::Eq,
            ">" => Cmp::Gt,
            ">=" => Cmp::Ge,
            "<" => Cmp::Lt,
            "<=" => Cmp::Le,
            _ => return Err(bad()),
        };
        pairs.push((cmp, *n));
    }
    match pairs.as_slice() {
        [_] => {}
        [(Cmp::Gt | Cmp::Ge, lo), (Cmp::Lt | Cmp::Le, hi)] if lo < hi => {}
        _ => return Err(bad()),
    }
    Ok(Cond::Numeric(pairs))
}
