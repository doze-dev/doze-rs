//! The search expression of `SEARCH('{AWS/EC2,InstanceId} MetricName="CPUUtilization"', 'Average', 300)`, read and matched against
//! the metrics this region holds. The syntax is AWS's (user guide, "CloudWatch search expression syntax"; memory, not recorded):
//!
//! - a schema `{Namespace,DimensionName,...}` keeps the metrics of that namespace with exactly those dimension names;
//! - `Name=value` keeps metrics whose `MetricName`, `Namespace` or dimension `Name` has that value;
//! - a bare term is a partial match on the namespace, the metric name, or any dimension name or value; a term in double quotes must
//!   match one of them whole;
//! - terms side by side must all match (`AND`), and `OR`, `NOT` and brackets combine them.
//!
//! Departure: AWS matches partial terms on the words of a name (split at punctuation and case changes); here a partial term is a
//! case-insensitive part of the name. Fuzzed by `fuzz_metric_math`, with the expression it sits in.

use crate::model::Series;

/// How deep brackets may nest.
const MAX_DEPTH: usize = 32;

/// A search, read.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Query {
    Schema(String, Vec<String>),
    Prop(String, String, bool),
    Term(String, bool),
    And(Vec<Query>),
    Or(Box<Query>, Box<Query>),
    Not(Box<Query>),
}

#[derive(Clone, Debug, PartialEq)]
enum Word {
    Text(String, bool),
    Schema(Vec<(String, bool)>),
    Eq,
    Open,
    Close,
}

/// Reads a search expression.
///
/// # Errors
/// What is wrong, in a sentence.
pub(crate) fn parse(text: &str) -> Result<Query, String> {
    let words = words(text)?;
    let mut p = Reader { words, at: 0 };
    let q = p.or(0)?;
    if p.at < p.words.len() {
        return Err("unexpected ')' in the search expression".to_owned());
    }
    Ok(q)
}

fn words(text: &str) -> Result<Vec<Word>, String> {
    let mut out = Vec::new();
    let mut it = text.chars().peekable();
    while let Some(&c) = it.peek() {
        match c {
            c if c.is_whitespace() => {
                it.next();
            }
            '(' | ')' | '=' => {
                it.next();
                out.push(match c {
                    '(' => Word::Open,
                    ')' => Word::Close,
                    _ => Word::Eq,
                });
            }
            '{' => {
                it.next();
                out.push(Word::Schema(schema(&mut it)?));
            }
            _ => out.push(term(&mut it)?),
        }
    }
    Ok(out)
}

type Chars<'a> = std::iter::Peekable<std::str::Chars<'a>>;

fn schema(it: &mut Chars<'_>) -> Result<Vec<(String, bool)>, String> {
    let mut parts = vec![(String::new(), false)];
    loop {
        match it.next() {
            None => return Err("a schema '{' is not closed".to_owned()),
            Some('}') => break,
            Some(',') => parts.push((String::new(), false)),
            Some('"') => {
                let s = quoted(it)?;
                if let Some(last) = parts.last_mut() {
                    *last = (s, true);
                }
            }
            Some(c) if c.is_whitespace() => {}
            Some(c) => {
                if let Some(last) = parts.last_mut() {
                    last.0.push(c);
                }
            }
        }
    }
    Ok(parts.into_iter().filter(|(p, _)| !p.is_empty()).collect())
}

fn quoted(it: &mut Chars<'_>) -> Result<String, String> {
    let mut s = String::new();
    loop {
        match it.next() {
            None => return Err("a quoted term is not closed".to_owned()),
            Some('\\') => s.push(it.next().ok_or("a quoted term is not closed")?),
            Some('"') => return Ok(s),
            Some(c) => s.push(c),
        }
    }
}

fn term(it: &mut Chars<'_>) -> Result<Word, String> {
    if it.peek() == Some(&'"') {
        it.next();
        return Ok(Word::Text(quoted(it)?, true));
    }
    let mut s = String::new();
    while let Some(&c) = it.peek() {
        if c.is_whitespace() || matches!(c, '(' | ')' | '=' | '{' | '"') {
            break;
        }
        s.push(c);
        it.next();
    }
    if s.is_empty() {
        return Err("unexpected character in the search expression".to_owned());
    }
    Ok(Word::Text(s, false))
}

struct Reader {
    words: Vec<Word>,
    at: usize,
}

impl Reader {
    fn or(&mut self, depth: usize) -> Result<Query, String> {
        if depth > MAX_DEPTH {
            return Err("the search expression nests too deeply".to_owned());
        }
        let left = self.and(depth)?;
        if self.keyword("OR") {
            let right = self.or(depth + 1)?;
            return Ok(Query::Or(Box::new(left), Box::new(right)));
        }
        Ok(left)
    }

    fn keyword(&mut self, k: &str) -> bool {
        if matches!(self.words.get(self.at), Some(Word::Text(t, false)) if t == k) {
            self.at += 1;
            return true;
        }
        false
    }

    fn and(&mut self, depth: usize) -> Result<Query, String> {
        let mut all = Vec::new();
        loop {
            match self.words.get(self.at) {
                None | Some(Word::Close) => break,
                Some(Word::Text(t, false)) if t == "OR" => break,
                _ => {}
            }
            if self.keyword("AND") {
                continue;
            }
            all.push(self.one(depth)?);
        }
        if all.is_empty() {
            return Err("the search expression has an empty part".to_owned());
        }
        Ok(if all.len() == 1 {
            all.swap_remove(0)
        } else {
            Query::And(all)
        })
    }

    fn one(&mut self, depth: usize) -> Result<Query, String> {
        if depth > MAX_DEPTH {
            return Err("the search expression nests too deeply".to_owned());
        }
        if self.keyword("NOT") {
            return Ok(Query::Not(Box::new(self.one(depth + 1)?)));
        }
        let w = self.words.get(self.at).cloned();
        self.at += 1;
        match w {
            Some(Word::Open) => {
                let q = self.or(depth + 1)?;
                if self.words.get(self.at) != Some(&Word::Close) {
                    return Err("a '(' in the search expression is not closed".to_owned());
                }
                self.at += 1;
                Ok(q)
            }
            Some(Word::Schema(parts)) => {
                let mut names = parts.into_iter().map(|(p, _)| p);
                let ns = names.next().ok_or("a schema names no namespace")?;
                Ok(Query::Schema(ns, names.collect()))
            }
            Some(Word::Text(t, exact)) => {
                if self.words.get(self.at) == Some(&Word::Eq) {
                    self.at += 1;
                    let Some(Word::Text(v, ve)) = self.words.get(self.at).cloned() else {
                        return Err(format!("'{t}=' has no value"));
                    };
                    self.at += 1;
                    return Ok(Query::Prop(t, v, ve));
                }
                Ok(Query::Term(t, exact))
            }
            _ => Err("unexpected word in the search expression".to_owned()),
        }
    }
}

/// Whether `s` is a part of `whole` (case ignored), or all of it when `exact`.
fn hits(whole: &str, s: &str, exact: bool) -> bool {
    if exact {
        whole == s
    } else {
        whole.to_ascii_lowercase().contains(&s.to_ascii_lowercase())
    }
}

/// Whether the metric matches the search.
pub(crate) fn matches(q: &Query, m: &Series) -> bool {
    match q {
        Query::Schema(ns, dims) => {
            m.namespace == *ns
                && m.dims.len() == dims.len()
                && dims.iter().all(|d| m.dims.iter().any(|(n, _)| n == d))
        }
        Query::Prop(key, value, exact) => match key.as_str() {
            "MetricName" => hits(&m.name, value, *exact),
            "Namespace" => hits(&m.namespace, value, *exact),
            _ => m
                .dims
                .iter()
                .any(|(n, v)| n == key && hits(v, value, *exact)),
        },
        Query::Term(t, exact) => {
            hits(&m.namespace, t, *exact)
                || hits(&m.name, t, *exact)
                || m.dims
                    .iter()
                    .any(|(n, v)| hits(n, t, *exact) || hits(v, t, *exact))
        }
        Query::And(all) => all.iter().all(|q| matches(q, m)),
        Query::Or(a, b) => matches(a, m) || matches(b, m),
        Query::Not(q) => !matches(q, m),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn series(ns: &str, name: &str, dims: &[(&str, &str)]) -> Series {
        Series {
            no: 0,
            namespace: ns.to_owned(),
            name: name.to_owned(),
            dims: dims
                .iter()
                .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
                .collect(),
            last_ms: 0,
        }
    }

    #[test]
    fn a_schema_keeps_the_metrics_with_exactly_its_dimensions() {
        let q = parse("{AWS/EC2,InstanceId} MetricName=\"CPUUtilization\"").unwrap();
        assert!(matches(
            &q,
            &series("AWS/EC2", "CPUUtilization", &[("InstanceId", "i-1")])
        ));
        assert!(!matches(&q, &series("AWS/EC2", "CPUUtilization", &[])));
        assert!(!matches(
            &q,
            &series("AWS/EC2", "CPUUtilizationX", &[("InstanceId", "i-1")])
        ));
        assert!(!matches(
            &q,
            &series(
                "AWS/EC2",
                "CPUUtilization",
                &[("InstanceId", "i"), ("Type", "t")]
            )
        ));
    }

    #[test]
    fn terms_combine_with_and_or_not() {
        let m = series("app", "Errors", &[("service", "orders")]);
        assert!(matches(&parse("errors orders").unwrap(), &m));
        assert!(matches(&parse("nothing OR order").unwrap(), &m));
        assert!(!matches(&parse("errors NOT orders").unwrap(), &m));
        assert!(matches(&parse("(x OR y) OR service=orders").unwrap(), &m));
        assert!(!matches(&parse("\"Error\"").unwrap(), &m));
        for bad in ["(a", "{ns", "a=", "\"open", ")", "a () b"] {
            assert!(parse(bad).is_err(), "{bad}");
        }
    }
}
