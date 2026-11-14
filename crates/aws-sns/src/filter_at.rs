//! Where in a filter policy a mistake is, worded as AWS words it: SNS reads policies with Jackson, and a refusal found while reading
//! carries Jackson's location. Recorded on AWS (2026-11-15):
//!
//! ```text
//! Unrecognized match type nope\n at [Source: (String)"{"kind":[{"nope":1}]}"; line: 1, column: 19]
//! ```
//!
//! The source is the policy compacted (SNS re-serializes what it was sent: the caller's `{"kind": [...]}` came back without spaces),
//! and the column is where the reader stands once it has read the token the mistake is about: past the whole of a number, `true`,
//! `false` or `null`, past only the opening quote of a string (Jackson reads strings lazily), past the bracket of an array or object.

use serde_json::Value;

/// The most of the source Jackson quotes. (memory: Jackson 2.9's `MAX_CONTENT_SNIPPET` and its `[truncated N chars]` tail)
const MAX_SNIPPET: usize = 500;

/// A refusal found while reading a policy, and the value it is about (by address, for finding its place in the text afterwards).
#[derive(Debug)]
pub(crate) struct Fault {
    pub(crate) message: String,
    at: Option<usize>,
}

#[derive(Debug, PartialEq)]
enum Step {
    Key(String),
    Index(usize),
}

impl Fault {
    /// A refusal about `value`.
    pub(crate) fn at(message: impl Into<String>, value: &Value) -> Self {
        Self {
            message: message.into(),
            at: Some(std::ptr::from_ref(value) as usize),
        }
    }

    /// A refusal about the policy as a whole, which carries no location. (memory: which refusals AWS words without one)
    pub(crate) fn plain(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            at: None,
        }
    }

    /// The words of the refusal, with Jackson's location when it has one. `root` is what `text` parsed to.
    pub(crate) fn located(self, text: &str, root: &Value) -> String {
        let Some(addr) = self.at else {
            return self.message;
        };
        let source = crate::sub_attrs::compact(text);
        let column = path_to(root, addr).and_then(|path| column(&source, &path));
        let Some(column) = column else {
            return self.message;
        };
        let count = source.chars().count();
        let snippet = if count > MAX_SNIPPET {
            let head: String = source.chars().take(MAX_SNIPPET).collect();
            format!("{head}\"[truncated {} chars]", count - MAX_SNIPPET)
        } else {
            format!("{source}\"")
        };
        format!(
            "{}\n at [Source: (String)\"{snippet}; line: 1, column: {column}]",
            self.message
        )
    }
}

/// The keys and indexes from `root` to the value at `addr`.
fn path_to(root: &Value, addr: usize) -> Option<Vec<Step>> {
    if std::ptr::from_ref(root) as usize == addr {
        return Some(Vec::new());
    }
    match root {
        Value::Object(m) => m.iter().find_map(|(k, v)| {
            path_to(v, addr).map(|mut p| {
                p.insert(0, Step::Key(k.clone()));
                p
            })
        }),
        Value::Array(a) => a.iter().enumerate().find_map(|(i, v)| {
            path_to(v, addr).map(|mut p| {
                p.insert(0, Step::Index(i));
                p
            })
        }),
        _ => None,
    }
}

/// Jackson's column once it has read the token at `path` in the compact `source`.
fn column(source: &str, path: &[Step]) -> Option<usize> {
    let b = source.as_bytes();
    let mut i = 0;
    for step in path {
        i = child(b, i, step)?;
    }
    let read = match b.get(i)? {
        b'"' | b'[' | b'{' => i + 1,
        _ => skip(b, i)?,
    };
    Some(source.get(..read)?.chars().count() + 1)
}

/// Where the value at `step` inside the container starting at `i` begins.
fn child(b: &[u8], i: usize, step: &Step) -> Option<usize> {
    let object = *b.get(i)? == b'{';
    let mut j = i + 1;
    let mut index = 0;
    while *b.get(j)? != b']' && *b.get(j)? != b'}' {
        let here = if object {
            let key_end = skip(b, j)?;
            let key: String = serde_json::from_slice(b.get(j..key_end)?).ok()?;
            j = key_end + 1;
            *step == Step::Key(key)
        } else {
            *step == Step::Index(index)
        };
        if here {
            return Some(j);
        }
        j = skip(b, j)?;
        if b.get(j) == Some(&b',') {
            j += 1;
        }
        index += 1;
    }
    None
}

/// The end of the value starting at `i` (compact text: no whitespace outside strings).
fn skip(b: &[u8], i: usize) -> Option<usize> {
    match b.get(i)? {
        b'"' => {
            let mut j = i + 1;
            loop {
                match b.get(j)? {
                    b'\\' => j += 2,
                    b'"' => return Some(j + 1),
                    _ => j += 1,
                }
            }
        }
        b'[' | b'{' => {
            let mut j = i + 1;
            while *b.get(j)? != b']' && *b.get(j)? != b'}' {
                j = skip(b, j)?;
                if b.get(j) == Some(&b':') || b.get(j) == Some(&b',') {
                    j += 1;
                }
            }
            Some(j + 1)
        }
        _ => {
            let len = b
                .get(i..)?
                .iter()
                .position(|c| matches!(c, b',' | b']' | b'}'));
            Some(i + len.unwrap_or(b.len() - i))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str, pick: impl Fn(&Value) -> &Value) -> String {
        let root: Value = serde_json::from_str(text).unwrap();
        Fault::at("m", pick(&root)).located(text, &root)
    }

    #[test]
    fn the_column_is_where_jackson_stands_as_recorded() {
        assert_eq!(
            at(r#"{"kind": [{"nope": 1}]}"#, |v| &v["kind"][0]["nope"]),
            "m\n at [Source: (String)\"{\"kind\":[{\"nope\":1}]}\"; line: 1, column: 19]"
        );
        assert_eq!(
            at(
                r#"{"kind": [{"exists": "yes"}]}"#,
                |v| &v["kind"][0]["exists"]
            ),
            "m\n at [Source: (String)\"{\"kind\":[{\"exists\":\"yes\"}]}\"; line: 1, column: 21]"
        );
        assert_eq!(
            at("[1]", |v| v),
            "m\n at [Source: (String)\"[1]\"; line: 1, column: 2]"
        );
    }

    #[test]
    fn keys_are_found_in_the_order_written_not_the_order_kept() {
        let text = r#"{"z":["a"],"a":[{"x":"y"}]}"#;
        assert!(at(text, |v| &v["a"][0]["x"]).ends_with("column: 23]"));
        assert!(at(text, |v| &v["z"]).ends_with("column: 7]"));
    }

    #[test]
    fn a_long_source_is_cut_as_jackson_cuts_it() {
        let text = format!(r#"{{"k":["{}",{{"nope":1}}]}}"#, "x".repeat(600));
        let got = at(&text, |v| &v["k"][1]["nope"]);
        assert!(
            got.contains("\"[truncated 121 chars]; line: 1, column: 619]"),
            "{got}"
        );
    }

    #[test]
    fn a_plain_fault_has_no_location() {
        assert_eq!(Fault::plain("p").located("{}", &Value::Null), "p");
    }
}
