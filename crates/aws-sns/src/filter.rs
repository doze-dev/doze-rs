//! Subscription filter policies, read and checked. A policy is an object whose keys name message attributes (or, with the scope
//! `MessageBody`, fields of the JSON body, to any depth); each key holds an array of conditions, any of which may match, and every key
//! must match. A key `$or` holds an array of objects, any of which may match.
//!
//! The operators are the ones SNS documents: exact values (strings, numbers, `true`, `false`, `null`), `prefix`, `suffix`,
//! `equals-ignore-case`, `anything-but` (a value, a list, or a `prefix` / `suffix`), `numeric`, `exists` and `cidr`. The words of the
//! refusals are from memory except the one for text that is not JSON, which carries the parser's message as Jackson worded it (recorded
//! 2026-11-09, for the one mistake the scenario makes).

use std::net::IpAddr;

use serde_json::Value;

/// The most a policy may fan out to: the product of the lengths of its arrays.
const MAX_COMBINATIONS: u64 = 150;
/// The most attribute names a policy on message attributes may have.
const MAX_ATTRIBUTE_NAMES: usize = 5;

/// A comparison for a number.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Cmp {
    Eq,
    Gt,
    Ge,
    Lt,
    Le,
}

/// One condition of an array.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Cond {
    Str(String),
    Num(f64),
    Bool(bool),
    Null,
    Prefix(String),
    Suffix(String),
    EqualsIgnoreCase(String),
    /// Anything but these exact values.
    ButExact(Vec<Cond>),
    ButPrefix(String),
    ButSuffix(String),
    /// Every comparison must hold (`[">", 1, "<=", 5]`).
    Numeric(Vec<(Cmp, f64)>),
    Exists(bool),
    Cidr(IpAddr, u8),
}

/// What one key holds.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Node {
    Leaf(Vec<Cond>),
    /// A field of a body that is itself an object.
    Nested(Rules),
}

/// A set of keys that must all match, and branches of which one must.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Rules {
    pub(crate) keys: Vec<(String, Node)>,
    pub(crate) or: Option<Vec<Rules>>,
}

// fuzz:parser fuzz_filter_policy
/// Reads `text` as a policy for the given scope.
///
/// # Errors
/// What is wrong with it, worded for `Invalid parameter: Attributes Reason: FilterPolicy: <this>`.
pub(crate) fn parse(text: &str, body_scope: bool) -> Result<Rules, String> {
    let value: Value = serde_json::from_str(text).map_err(|e| json_failure(text, &e))?;
    let Value::Object(map) = &value else {
        return Err("Filter policy must be a JSON object".into());
    };
    if !body_scope && map.keys().filter(|k| *k != "$or").count() > MAX_ATTRIBUTE_NAMES {
        return Err("Filter policy can not have more than 5 keys".into());
    }
    let rules = rules(map, body_scope, 0)?;
    if combinations(&rules) > MAX_COMBINATIONS {
        return Err("Filter policy is too complex".into());
    }
    Ok(rules)
}

/// Jackson's sentence for the commonest mistake (a key that is not quoted), the serde one for the rest.
fn json_failure(text: &str, e: &serde_json::Error) -> String {
    let at = |column: usize| {
        text.lines()
            .nth(e.line().saturating_sub(1))
            .and_then(|l| l.chars().nth(column.saturating_sub(1)))
    };
    if e.to_string().starts_with("key must be a string")
        && let Some(c) = at(e.column())
    {
        return format!(
            "failed to parse JSON. Unexpected character ('{c}' (code {})): was expecting double-quote to start field name\n at [Source: REDACTED (`StreamReadFeature.INCLUDE_SOURCE_IN_LOCATION` disabled); line: {}, column: {}]",
            u32::from(c),
            e.line(),
            e.column()
        );
    }
    format!("failed to parse JSON. {e}")
}

fn rules(map: &serde_json::Map<String, Value>, body: bool, depth: u32) -> Result<Rules, String> {
    if depth > 5 {
        return Err("Filter policy is too deeply nested".into());
    }
    let mut out = Rules::default();
    for (key, value) in map {
        if key == "$or" {
            out.or = Some(branches(value, body, depth)?);
        } else {
            out.keys.push((key.clone(), node(key, value, body, depth)?));
        }
    }
    Ok(out)
}

fn branches(value: &Value, body: bool, depth: u32) -> Result<Vec<Rules>, String> {
    let Value::Array(list) = value else {
        return Err("$or must be an array of objects".into());
    };
    list.iter()
        .map(|b| match b {
            Value::Object(m) => rules(m, body, depth + 1),
            _ => Err("$or must be an array of objects".into()),
        })
        .collect()
}

fn node(key: &str, value: &Value, body: bool, depth: u32) -> Result<Node, String> {
    match value {
        Value::Array(list) => Ok(Node::Leaf(
            list.iter().map(cond).collect::<Result<Vec<_>, _>>()?,
        )),
        Value::Object(m) if body => Ok(Node::Nested(rules(m, body, depth + 1)?)),
        _ => Err(format!(
            "Attribute name '{key}' must be followed by an array"
        )),
    }
}

fn cond(v: &Value) -> Result<Cond, String> {
    match v {
        Value::String(s) => Ok(Cond::Str(s.clone())),
        Value::Number(n) => n
            .as_f64()
            .map(Cond::Num)
            .ok_or_else(|| "Match value must be String, number, true, false, or null".into()),
        Value::Bool(b) => Ok(Cond::Bool(*b)),
        Value::Null => Ok(Cond::Null),
        Value::Object(m) => operator(m),
        Value::Array(_) => Err("Match value must be String, number, true, false, or null".into()),
    }
}

fn operator(m: &serde_json::Map<String, Value>) -> Result<Cond, String> {
    let mut it = m.iter();
    let (Some((name, arg)), None) = (it.next(), it.next()) else {
        return Err("Match pattern must have exactly one operator".into());
    };
    let text = |what: &str| {
        arg.as_str()
            .map(str::to_owned)
            .ok_or_else(|| format!("{what} match pattern must be a string"))
    };
    match name.as_str() {
        "prefix" => text("prefix").map(Cond::Prefix),
        "suffix" => text("suffix").map(Cond::Suffix),
        "equals-ignore-case" => text("equals-ignore-case").map(Cond::EqualsIgnoreCase),
        "exists" => arg
            .as_bool()
            .map(Cond::Exists)
            .ok_or_else(|| "exists match pattern must be either true or false.".into()),
        "numeric" => numeric(arg),
        "cidr" => cidr(arg),
        "anything-but" => anything_but(arg),
        other => Err(format!("Unrecognized match type {other}")),
    }
}

fn numeric(arg: &Value) -> Result<Cond, String> {
    let bad = || "Invalid member in numeric match".to_owned();
    let Value::Array(items) = arg else {
        return Err("Value of numeric must be an array".into());
    };
    if items.is_empty() || items.len() % 2 != 0 || items.len() > 4 {
        return Err(bad());
    }
    let mut out = Vec::new();
    for pair in items.chunks(2) {
        let (Some(op), Some(n)) = (
            pair.first().and_then(Value::as_str),
            pair.get(1).and_then(Value::as_f64),
        ) else {
            return Err(bad());
        };
        let cmp = match op {
            "=" => Cmp::Eq,
            ">" => Cmp::Gt,
            ">=" => Cmp::Ge,
            "<" => Cmp::Lt,
            "<=" => Cmp::Le,
            other => return Err(format!("Unrecognized numeric range operator: {other}")),
        };
        out.push((cmp, n));
    }
    Ok(Cond::Numeric(out))
}

fn cidr(arg: &Value) -> Result<Cond, String> {
    let bad = || "Malformed CIDR, one '/' required".to_owned();
    let (addr, bits) = arg
        .as_str()
        .and_then(|s| s.split_once('/'))
        .ok_or_else(bad)?;
    let addr: IpAddr = addr.parse().map_err(|_| bad())?;
    let bits: u8 = bits.parse().map_err(|_| bad())?;
    let widest = if addr.is_ipv4() { 32 } else { 128 };
    if bits > widest {
        return Err(bad());
    }
    Ok(Cond::Cidr(addr, bits))
}

fn anything_but(arg: &Value) -> Result<Cond, String> {
    match arg {
        Value::Object(m) => match operator(m)? {
            Cond::Prefix(p) => Ok(Cond::ButPrefix(p)),
            Cond::Suffix(s) => Ok(Cond::ButSuffix(s)),
            _ => Err("Unsupported anything-but pattern".into()),
        },
        Value::Array(items) => Ok(Cond::ButExact(
            items.iter().map(exact).collect::<Result<Vec<_>, _>>()?,
        )),
        one => Ok(Cond::ButExact(vec![exact(one)?])),
    }
}

fn exact(v: &Value) -> Result<Cond, String> {
    match cond(v)? {
        c @ (Cond::Str(_) | Cond::Num(_)) => Ok(c),
        _ => Err("Unsupported anything-but pattern".into()),
    }
}

/// How many combinations of values the policy stands for: the product of the lengths of its arrays.
fn combinations(r: &Rules) -> u64 {
    let keys: u64 = r
        .keys
        .iter()
        .map(|(_, n)| match n {
            Node::Leaf(c) => (c.len() as u64).max(1),
            Node::Nested(inner) => combinations(inner),
        })
        .fold(1, u64::saturating_mul);
    let or: u64 =
        r.or.as_ref()
            .map_or(1, |b| b.iter().map(combinations).sum::<u64>().max(1));
    keys.saturating_mul(or)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_operators_are_read() {
        let p = r#"{"a":["x",1,true,null,{"prefix":"p"},{"suffix":"s"},{"equals-ignore-case":"Q"},
            {"anything-but":["u",2]},{"anything-but":{"prefix":"no"}},{"numeric":[">",1,"<=",5]},
            {"exists":false},{"cidr":"10.0.0.0/24"}]}"#;
        let rules = parse(p, false).unwrap();
        let Some((_, Node::Leaf(conds))) = rules.keys.first() else {
            panic!("a leaf");
        };
        assert_eq!(conds.len(), 12);
        assert_eq!(
            conds.get(9),
            Some(&Cond::Numeric(vec![(Cmp::Gt, 1.0), (Cmp::Le, 5.0)]))
        );
    }

    #[test]
    fn a_body_policy_nests_and_an_attribute_policy_does_not() {
        assert!(parse(r#"{"a":{"b":["x"]}}"#, true).is_ok());
        assert!(parse(r#"{"a":{"b":["x"]}}"#, false).is_err());
        assert!(parse(r#"{"$or":[{"a":["x"]},{"b":["y"]}]}"#, false).is_ok());
        assert!(parse(r#"{"$or":{"a":["x"]}}"#, false).is_err());
    }

    #[test]
    fn what_is_wrong_is_said() {
        let err = |p: &str| parse(p, false).unwrap_err();
        assert!(err("[]").contains("JSON object"));
        assert!(err(r#"{"a":[{"foo":1}]}"#).contains("Unrecognized match type foo"));
        assert!(err(r#"{"a":[{"exists":"yes"}]}"#).contains("exists match pattern"));
        assert!(err(r#"{"a":[{"numeric":["~",1]}]}"#).contains("numeric range operator"));
        assert!(err(r#"{"a":[[1]]}"#).contains("Match value"));
        assert!(err(r#"{"a":[{"cidr":"10.0.0.0"}]}"#).contains("CIDR"));
        assert!(err(r#"{"a":"x"}"#).contains("must be followed by an array"));
        assert!(err(r#"{"a":[1],"b":[1],"c":[1],"d":[1],"e":[1],"f":[1]}"#).contains("5 keys"));
    }

    #[test]
    fn a_policy_that_fans_out_too_far_is_too_complex() {
        let ten = "[1,2,3,4,5,6,7,8,9,10]";
        let big = format!(r#"{{"a":{ten},"b":{ten},"c":{ten}}}"#);
        assert!(parse(&big, false).unwrap_err().contains("too complex"));
        let fine = format!(r#"{{"a":{ten},"b":{ten}}}"#);
        assert!(parse(&fine, false).is_ok());
    }

    #[test]
    fn text_that_is_not_json_is_worded_as_jackson_worded_it() {
        let e = parse("{not json", false).unwrap_err();
        assert_eq!(
            e,
            "failed to parse JSON. Unexpected character ('n' (code 110)): was expecting double-quote to start field name\n at [Source: REDACTED (`StreamReadFeature.INCLUDE_SOURCE_IN_LOCATION` disabled); line: 1, column: 2]"
        );
    }
}
