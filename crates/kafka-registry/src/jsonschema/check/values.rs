//! The values of keywords that are not schemas: types, counts, names, enums, patterns.

use super::*;

pub(super) fn doc_draft(c: &Checker<'_>, doc: usize) -> Draft {
    c.docs.doc(doc).map_or(Draft::D7, |d| d.draft)
}

pub(super) fn expected(shape: Shape) -> &'static str {
    match shape {
        Shape::Str | Shape::Pattern => "must be a string",
        Shape::Bool => "must be a boolean",
        Shape::Array | Shape::Required | Shape::Enum | Shape::Schemas => "must be an array",
        Shape::Num | Shape::Count | Shape::Positive => "must be a number",
        _ => "must be an object",
    }
}

pub(super) fn number(shape: Shape, raw: &str) -> Result<(), &'static str> {
    let n = Dec::parse(raw).ok_or("must be a number")?;
    match shape {
        Shape::Count if !n.is_integer() || n.is_negative() => Err("must be a non-negative integer"),
        Shape::Positive if n.is_negative() || n.is_zero() => Err("must be greater than 0"),
        _ => Ok(()),
    }
}

pub(super) fn types(v: &Json) -> Result<(), String> {
    fn one(t: &Json) -> Result<&str, String> {
        match t {
            Json::Str(s) if TYPES.contains(&s.as_str()) => Ok(s.as_str()),
            Json::Str(s) => Err(format!("\"{s}\" is not a type")),
            _ => Err("a type must be a string".to_owned()),
        }
    }
    match v {
        Json::Array(items) => {
            if items.is_empty() {
                return Err("must not be empty".to_owned());
            }
            let mut seen = Vec::new();
            for t in items {
                let t = one(t)?;
                if seen.contains(&t) {
                    return Err(format!("\"{t}\" is named twice"));
                }
                seen.push(t);
            }
            Ok(())
        }
        _ => one(v).map(|_| ()),
    }
}

/// An array of distinct strings (`required`, a dependency); before draft 6 it may not be empty.
pub(super) fn strings(items: &[Json], non_empty: bool) -> Result<(), String> {
    if non_empty && items.is_empty() {
        return Err("must not be empty".to_owned());
    }
    let mut seen: Vec<&str> = Vec::new();
    for x in items {
        let s = x.as_str().ok_or("must be an array of strings")?;
        if seen.contains(&s) {
            return Err(format!("\"{s}\" is named twice"));
        }
        seen.push(s);
    }
    Ok(())
}

/// `enum`: distinct values, and at least one until 2019-09.
pub(super) fn enumeration(items: &[Json], draft: Draft) -> Result<(), String> {
    if draft >= Draft::D2019 {
        return Ok(());
    }
    if items.is_empty() {
        return Err("must not be empty".to_owned());
    }
    let keys: Vec<String> = items.iter().map(value_key).collect();
    for (i, k) in keys.iter().enumerate() {
        if keys.iter().skip(i + 1).any(|o| o == k) {
            return Err("values must be distinct".to_owned());
        }
    }
    Ok(())
}

/// A value as JSON Schema compares values: object keys in order, numbers by value.
pub(super) fn value_key(v: &Json) -> String {
    match v {
        Json::Raw(r) => Dec::parse(r).map_or_else(|| r.clone(), |d| format!("{d:?}")),
        Json::Str(s) => format!("{s:?}"),
        Json::Array(a) => format!(
            "[{}]",
            a.iter().map(value_key).collect::<Vec<_>>().join(",")
        ),
        Json::Object(_) => {
            let mut m: Vec<(&str, String)> = members(v)
                .into_iter()
                .map(|(k, x)| (k, value_key(x)))
                .collect();
            m.sort();
            format!(
                "{{{}}}",
                m.iter()
                    .map(|(k, x)| format!("{k:?}:{x}"))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        }
    }
}

/// Whether `p` is a regular expression, judged by what makes one malformed in Java's `Pattern` (Confluent's): an
/// unclosed group or class, a `)` with no group, a trailing backslash, a quantifier with nothing to repeat. A pattern
/// these pass and Java would still refuse is taken (it is compared as text, never run here).
pub(crate) fn regex(p: &str) -> Result<(), String> {
    let bad = |what: &str| Err(format!("\"{p}\" is not a regular expression: {what}"));
    let (mut groups, mut class, mut atom) = (0_usize, 0_usize, false);
    let mut chars = p.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if chars.next().is_none() {
                    return bad("a trailing backslash");
                }
                atom = true;
            }
            '[' => {
                class += 1;
                if chars.peek() == Some(&'^') {
                    chars.next();
                }
                if class == 1 && chars.peek() == Some(&']') {
                    chars.next();
                }
            }
            ']' if class > 0 => {
                class -= 1;
                atom = true;
            }
            _ if class > 0 => {}
            '(' => {
                groups += 1;
                atom = false;
                if chars.peek() == Some(&'?') {
                    chars.next();
                }
            }
            ')' => {
                if groups == 0 {
                    return bad("an unmatched ')'");
                }
                groups -= 1;
                atom = true;
            }
            '|' => atom = false,
            '*' | '+' | '?' if !atom => return bad("a quantifier with nothing to repeat"),
            '*' | '+' | '?' => {
                if matches!(chars.peek(), Some('?' | '+')) {
                    chars.next();
                }
            }
            '^' | '$' => atom = false,
            _ => atom = true,
        }
    }
    if class > 0 {
        return bad("an unclosed '['");
    }
    if groups > 0 {
        return bad("an unclosed '('");
    }
    Ok(())
}
