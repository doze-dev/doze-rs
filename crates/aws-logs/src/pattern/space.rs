//! Space-delimited patterns: `[ip, user, ..., status = 4* || status = 5*, bytes]`. The message is cut into fields at blanks (a
//! quoted or bracketed run is one field); each named field of the pattern takes one, `...` takes any number, and a condition on
//! a field must hold.

use super::{PatternError, glob};

type R<T> = Result<T, PatternError>;

#[derive(Debug)]
pub(crate) struct Fields {
    items: Vec<Item>,
}

#[derive(Debug)]
enum Item {
    Ellipsis,
    Field(String, Option<Cond>),
}

/// Conditions joined by `||`, each of which is conditions joined by `&&`.
#[derive(Debug)]
struct Cond(Vec<Vec<(Op, String)>>);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op {
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
}

pub(super) fn parse(text: &str) -> R<Fields> {
    let inner = text
        .strip_prefix('[')
        .and_then(|t| t.trim_end().strip_suffix(']'))
        .ok_or(PatternError::Invalid)?;
    if inner.contains('%') {
        return Err(PatternError::Regex);
    }
    let items = split_commas(inner)
        .iter()
        .map(|part| item(part.trim()))
        .collect::<R<Vec<_>>>()?;
    let ellipses = items.iter().filter(|i| matches!(i, Item::Ellipsis)).count();
    if items.is_empty() || ellipses > 1 {
        return Err(PatternError::Invalid);
    }
    Ok(Fields { items })
}

/// Splits at commas outside double quotes.
fn split_commas(text: &str) -> Vec<String> {
    let (mut parts, mut cur, mut quoted) = (Vec::new(), String::new(), false);
    for c in text.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                cur.push(c);
            }
            ',' if !quoted => parts.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    parts.push(cur);
    parts
}

fn item(part: &str) -> R<Item> {
    if part == "..." {
        return Ok(Item::Ellipsis);
    }
    let end = part
        .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$'))
        .unwrap_or(part.len());
    let (name, rest) = part.split_at(end);
    if name.is_empty() {
        return Err(PatternError::Invalid);
    }
    let rest = rest.trim();
    if rest.is_empty() {
        return Ok(Item::Field(name.to_owned(), None));
    }
    Ok(Item::Field(name.to_owned(), Some(cond(name, rest)?)))
}

/// `= 4* || = 5*`, or with the name repeated before each operator.
fn cond(name: &str, text: &str) -> R<Cond> {
    let mut or = Vec::new();
    for alt in text.split("||") {
        let mut and = Vec::new();
        for atom in alt.split("&&") {
            let atom = atom.trim();
            let atom = atom.strip_prefix(name).map_or(atom, str::trim_start);
            and.push(atom_of(atom)?);
        }
        or.push(and);
    }
    Ok(Cond(or))
}

fn atom_of(atom: &str) -> R<(Op, String)> {
    let (op, rest) = ["!=", "<=", ">=", "=", "<", ">"]
        .iter()
        .find_map(|p| atom.strip_prefix(p).map(|r| (*p, r)))
        .ok_or(PatternError::Invalid)?;
    let value = rest.trim();
    let value = value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .unwrap_or(value);
    if value.is_empty() {
        return Err(PatternError::Invalid);
    }
    let op = match op {
        "!=" => Op::Ne,
        "<=" => Op::Le,
        ">=" => Op::Ge,
        "<" => Op::Lt,
        ">" => Op::Gt,
        _ => Op::Eq,
    };
    Ok((op, value.to_owned()))
}

/// The fields of a message.
fn split_fields(message: &str) -> Vec<String> {
    let (mut out, mut cur, mut close) = (Vec::new(), String::new(), None);
    for c in message.chars() {
        match (close, c) {
            (Some(end), c) if c == end => {
                out.push(std::mem::take(&mut cur));
                close = None;
            }
            (Some(_), c) => cur.push(c),
            (None, '"') if cur.is_empty() => close = Some('"'),
            (None, '[') if cur.is_empty() => close = Some(']'),
            (None, c) if c.is_whitespace() => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            (None, c) => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn holds(op: Op, value: &str, field: &str) -> bool {
    if let (Ok(a), Ok(b)) = (field.parse::<f64>(), value.parse::<f64>()) {
        return match op {
            Op::Eq => (a - b).abs() < f64::EPSILON,
            Op::Ne => (a - b).abs() >= f64::EPSILON,
            Op::Lt => a < b,
            Op::Gt => a > b,
            Op::Le => a <= b,
            Op::Ge => a >= b,
        };
    }
    match op {
        Op::Eq => glob(value, field),
        Op::Ne => !glob(value, field),
        _ => false,
    }
}

impl Cond {
    fn holds(&self, field: &str) -> bool {
        self.0
            .iter()
            .any(|and| and.iter().all(|(op, v)| holds(*op, v, field)))
    }
}

impl Fields {
    /// The message's fields lined up with the pattern's items, if they line up.
    fn align<'m>(&self, fields: &'m [String]) -> Option<Vec<(&Item, &'m str)>> {
        let split = self.items.iter().position(|i| matches!(i, Item::Ellipsis));
        let (head, tail) = match split {
            Some(at) => (self.items.get(..at)?, self.items.get(at + 1..)?),
            None => (self.items.as_slice(), [].as_slice()),
        };
        if (split.is_none() && fields.len() != head.len()) || fields.len() < head.len() + tail.len()
        {
            return None;
        }
        let tail_from = fields.len() - tail.len();
        let pairs = head
            .iter()
            .zip(fields.iter())
            .chain(tail.iter().zip(fields.get(tail_from..)?.iter()));
        Some(pairs.map(|(i, f)| (i, f.as_str())).collect())
    }

    pub(crate) fn matches(&self, message: &str) -> bool {
        let fields = split_fields(message);
        self.align(&fields).is_some_and(|pairs| {
            pairs.iter().all(|(item, field)| match item {
                Item::Field(_, Some(cond)) => cond.holds(field),
                _ => true,
            })
        })
    }

    /// The value of the named field in a message.
    pub(crate) fn field(&self, message: &str, name: &str) -> Option<String> {
        let fields = split_fields(message);
        let pairs = self.align(&fields)?;
        pairs.iter().find_map(|(item, field)| match item {
            Item::Field(n, _) if n == name => Some((*field).to_owned()),
            _ => None,
        })
    }

    /// Every named field of a message, as `$name` and its value.
    pub(crate) fn named(&self, message: &str) -> Vec<(String, String)> {
        let fields = split_fields(message);
        self.align(&fields)
            .map(|pairs| {
                pairs
                    .iter()
                    .filter_map(|(item, field)| match item {
                        Item::Field(n, _) => Some((format!("${n}"), (*field).to_owned())),
                        Item::Ellipsis => None,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}
