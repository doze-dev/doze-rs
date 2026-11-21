//! What the Live tail page keeps of the records it reads (`/tail`, CON-1582): pure, so every rule is a table of cases.
//!
//! - **Key contains**, **value contains**: a part of the text, letters of either case (`ORD-1` finds `ord-17`). A value in the
//!   registry's wire format is matched as the JSON its schema reads it as, so `"sku":"A-1"` finds an Avro record.
//! - **Header**: a header of exactly that name; with a value, one whose value contains it.
//! - **JSON field equals**: a simple path (`customer.id`, `items[0].sku`, `items.0.sku`, an optional leading `$.`) into a
//!   value that is JSON; equal when the field is that string, or its JSON text is (`42`, `true`, `null`, `{"a":1}`). With no
//!   value asked for, the field being there is enough. An Avro union, which its JSON writes as `{"string": "rush"}`, is
//!   seen through: `note` equals `rush`, and `customer.id` reads `{"customer": {"shop.Customer": {"id": ..}}}`.
//! - A topic name pattern is a glob: `*` any run of characters, `?` one (`orders.*` follows `orders.eu` and `orders.us`).

/// One step of a path into JSON.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    Key(String),
    Index(usize),
}

/// The most characters a filter's text may have.
pub const MOST_TEXT: usize = 256;

/// `customer.id`, `items[0].sku`, `$.a.b`: the steps; `None` when it is not a path (empty, `a..b`, `a[x]`, too deep).
#[must_use]
pub fn path(text: &str) -> Option<Vec<Step>> {
    let t = text.trim();
    let t = t
        .strip_prefix("$.")
        .or_else(|| t.strip_prefix('$'))
        .unwrap_or(t);
    if t.is_empty() || t.chars().count() > MOST_TEXT {
        return None;
    }
    let mut out = Vec::new();
    for part in t.split('.') {
        let (name, mut rest) = part.find('[').map_or((part, ""), |i| part.split_at(i));
        if name.is_empty() && rest.is_empty() {
            return None;
        }
        if !name.is_empty() {
            out.push(match name.parse::<usize>() {
                Ok(n) if out.is_empty() => Step::Key(n.to_string()),
                Ok(n) => Step::Index(n),
                Err(_) => Step::Key(name.to_owned()),
            });
        }
        while let Some(inner) = rest.strip_prefix('[') {
            let end = inner.find(']')?;
            let n = inner.get(..end)?.parse::<usize>().ok()?;
            out.push(Step::Index(n));
            rest = inner.get(end + 1..)?;
        }
        if !rest.is_empty() {
            return None;
        }
    }
    (out.len() <= 32).then_some(out)
}

/// The value at `steps` in `v`, if there is one. A number in the path also reads an object's key of that name.
#[must_use]
pub fn at<'a>(v: &'a serde_json::Value, steps: &[Step]) -> Option<&'a serde_json::Value> {
    let mut here = v;
    for s in steps {
        let step = |h: &'a serde_json::Value| match (s, h) {
            (Step::Key(k), serde_json::Value::Object(m)) => m.get(k),
            (Step::Index(i), serde_json::Value::Array(a)) => a.get(*i),
            (Step::Index(i), serde_json::Value::Object(m)) => m.get(&i.to_string()),
            _ => None,
        };
        here = step(here).or_else(|| union(here).and_then(step))?;
    }
    Some(here)
}

/// The value inside an Avro union as its JSON says it (`{"string": "rush"}`, `{"shop.Customer": {..}}`): one key, a type.
fn union(v: &serde_json::Value) -> Option<&serde_json::Value> {
    match v {
        serde_json::Value::Object(m) if m.len() == 1 => m.values().next(),
        _ => None,
    }
}

/// Whether the JSON value `v` is what `want` says: the string itself, or its JSON text.
#[must_use]
pub fn equals(v: &serde_json::Value, want: &str) -> bool {
    let plain = match v {
        serde_json::Value::String(s) => s == want,
        other => serde_json::to_string(other).is_ok_and(|t| t == want.trim()),
    };
    plain || union(v).is_some_and(|inner| !inner.is_object() && equals(inner, want))
}

/// Whether `hay` contains `needle`, letters of either case (ASCII folding; other characters as they are).
#[must_use]
pub fn contains(hay: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    if needle.len() > hay.len() {
        return false;
    }
    let (h, n) = (hay.as_bytes(), needle.as_bytes());
    h.windows(n.len()).any(|w| w.eq_ignore_ascii_case(n))
}

/// Whether `name` fits the glob `pattern` (`*` any run, `?` one character).
#[must_use]
pub fn glob(pattern: &str, name: &str) -> bool {
    let (p, n): (Vec<char>, Vec<char>) = (pattern.chars().collect(), name.chars().collect());
    let (mut pi, mut ni) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while ni < n.len() {
        match p.get(pi) {
            Some('*') => {
                star = Some((pi, ni));
                pi += 1;
            }
            Some(c) if *c == '?' || Some(c) == n.get(ni) => {
                pi += 1;
                ni += 1;
            }
            _ => match star {
                Some((sp, sn)) => {
                    pi = sp + 1;
                    ni = sn + 1;
                    star = Some((sp, sn + 1));
                }
                None => return false,
            },
        }
    }
    p.get(pi..)
        .is_some_and(|rest| rest.iter().all(|c| *c == '*'))
}

/// Whether `pattern` can be a topic name pattern: the characters of a topic name, `*` and `?`, at most 249 of them.
#[must_use]
pub fn valid_pattern(pattern: &str) -> bool {
    !pattern.is_empty()
        && pattern.len() <= 249
        && pattern
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '*' | '?'))
}

/// The filters of one tail; all of them must hold.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filter {
    pub key: Option<String>,
    pub value: Option<String>,
    pub header: Option<String>,
    pub header_value: Option<String>,
    pub field: Option<Vec<Step>>,
    /// The text the field must equal; `None`: the field must be there.
    pub equals: Option<String>,
}

/// A record as the filters read it: its key and value as text (the value as its schema reads it, when it has one) and its
/// headers' names and values.
#[derive(Debug, Clone, Copy)]
pub struct Candidate<'a> {
    pub key: Option<&'a str>,
    pub value: Option<&'a str>,
    pub headers: &'a [(String, String)],
}

impl Filter {
    /// No filter is on: every record read is kept, and no text of it is looked at.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.key.is_none() && self.value.is_none() && self.header.is_none() && self.field.is_none()
    }

    /// Whether a filter needs the value's text (to decode a schema value only when one does).
    #[must_use]
    pub fn reads_value(&self) -> bool {
        self.value.is_some() || self.field.is_some()
    }

    /// Whether `c` passes every filter that is on.
    #[must_use]
    pub fn matches(&self, c: &Candidate<'_>) -> bool {
        if let Some(k) = &self.key
            && !c.key.is_some_and(|t| contains(t, k))
        {
            return false;
        }
        if let Some(v) = &self.value
            && !c.value.is_some_and(|t| contains(t, v))
        {
            return false;
        }
        if let Some(name) = &self.header {
            let want = self.header_value.as_deref().unwrap_or("");
            if !c
                .headers
                .iter()
                .any(|(k, v)| k == name && contains(v, want))
            {
                return false;
            }
        }
        if let Some(steps) = &self.field {
            let Some(json) = c
                .value
                .and_then(|t| serde_json::from_str::<serde_json::Value>(t).ok())
            else {
                return false;
            };
            match (at(&json, steps), &self.equals) {
                (None, _) => return false,
                (Some(v), Some(want)) if !equals(v, want) => return false,
                _ => {}
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c<'a>(key: &'a str, value: &'a str, headers: &'a [(String, String)]) -> Candidate<'a> {
        Candidate {
            key: Some(key),
            value: Some(value),
            headers,
        }
    }

    #[test]
    fn a_path_is_names_and_indexes_and_nothing_else() {
        use Step::{Index, Key};
        let k = |s: &str| Key(s.to_owned());
        assert_eq!(path("customer.id"), Some(vec![k("customer"), k("id")]));
        assert_eq!(
            path("$.items[0].sku"),
            Some(vec![k("items"), Index(0), k("sku")])
        );
        assert_eq!(
            path("items.2.sku"),
            Some(vec![k("items"), Index(2), k("sku")])
        );
        assert_eq!(path("m[1][2]"), Some(vec![k("m"), Index(1), Index(2)]));
        for bad in ["", "a..b", "a[x]", "a[1", "a[1]b", ".", "$."] {
            assert_eq!(path(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_json_field_equals_its_string_or_its_json_text() {
        let none: [(String, String); 0] = [];
        let v =
            r#"{"id":42,"customer":{"id":"c-7","vip":true},"items":[{"sku":"A-1"}],"note":null}"#;
        let f = |p: &str, eq: Option<&str>| Filter {
            field: path(p),
            equals: eq.map(str::to_owned),
            ..Filter::default()
        };
        assert!(f("id", Some("42")).matches(&c("k", v, &none)));
        assert!(f("customer.id", Some("c-7")).matches(&c("k", v, &none)));
        assert!(f("customer.vip", Some("true")).matches(&c("k", v, &none)));
        assert!(f("items[0].sku", Some("A-1")).matches(&c("k", v, &none)));
        assert!(f("note", Some("null")).matches(&c("k", v, &none)));
        assert!(f("customer", Some(r#"{"id":"c-7","vip":true}"#)).matches(&c("k", v, &none)));
        assert!(
            f("customer.id", None).matches(&c("k", v, &none)),
            "there is enough"
        );
        assert!(!f("customer.id", Some("c-8")).matches(&c("k", v, &none)));
        assert!(!f("missing", None).matches(&c("k", v, &none)));
        assert!(!f("id", Some("42")).matches(&c("k", "not json", &none)));
        let avro = r#"{"note":{"string":"rush"},"customer":{"shop.Customer":{"id":"c-7"}}}"#;
        assert!(
            f("note", Some("rush")).matches(&c("k", avro, &none)),
            "a union is seen through"
        );
        assert!(f("customer.id", Some("c-7")).matches(&c("k", avro, &none)));
        assert!(
            f("note.string", Some("rush")).matches(&c("k", avro, &none)),
            "or named"
        );
    }

    #[test]
    fn key_value_and_header_filters_all_hold() {
        let h = [("trace-id".to_owned(), "abc-123".to_owned())];
        let f = Filter {
            key: Some("ORD-1".into()),
            value: Some("paid".into()),
            header: Some("trace-id".into()),
            header_value: Some("ABC".into()),
            ..Filter::default()
        };
        assert!(f.matches(&c("ord-17", r#"{"state":"PAID"}"#, &h)));
        assert!(!f.matches(&c("ord-2", r#"{"state":"PAID"}"#, &h)));
        assert!(!f.matches(&c("ord-17", r#"{"state":"open"}"#, &h)));
        assert!(!f.matches(&c("ord-17", "paid", &[])));
        let named = Filter {
            header: Some("trace-id".into()),
            ..Filter::default()
        };
        assert!(named.matches(&c("", "", &h)), "a name alone");
        let null = Candidate {
            key: None,
            value: None,
            headers: &[],
        };
        assert!(Filter::default().matches(&null) && Filter::default().is_empty());
        assert!(!f.matches(&null));
    }

    #[test]
    fn a_glob_is_star_and_question_mark() {
        for (p, n, want) in [
            ("orders.*", "orders.eu", true),
            ("orders.*", "orders.", true),
            ("orders.*", "orders", false),
            ("*", "anything", true),
            ("*.dlq", "payments.dlq", true),
            ("*.dlq", "payments.dlq.x", false),
            ("o?ders", "orders", true),
            ("a*b*c", "aXbYc", true),
            ("a*b*c", "aXbY", false),
            ("orders", "orders", true),
            ("orders", "orders2", false),
        ] {
            assert_eq!(glob(p, n), want, "{p} {n}");
        }
        assert!(valid_pattern("orders.*") && !valid_pattern("") && !valid_pattern("a b"));
    }
}
