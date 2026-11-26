//! JSON laid out for the value viewer: pretty, one line per key or item, each container foldable. Read by hand rather
//! than through `serde_json::Value`, so keys keep the order the value has and numbers keep the digits it has (a
//! 20-digit id is not rounded, `1.50` is not `1.5`).

use std::collections::HashSet;

/// What a line holds after its key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tok {
    /// `{` or `[`: a container opens; its closing line is [`JsonLine::end`].
    Open(char),
    /// `}` or `]`.
    Close(char),
    /// An empty container, `{}` or `[]`.
    Empty(&'static str),
    /// A string, as written (quotes and escapes).
    Str(String),
    Num(String),
    /// `true`, `false` or `null`.
    Lit(String),
}

/// One line of the pretty value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonLine {
    pub depth: usize,
    /// The key, as written (quotes and escapes), for a member of an object.
    pub key: Option<String>,
    pub tok: Tok,
    /// A comma follows (an opening line has none: its closing line has the container's).
    pub comma: bool,
    /// An opening line's closing line, and how many keys or items it holds.
    pub end: usize,
    pub count: usize,
    /// An opening line's first members in one line (`"id": 12, "status": "paid"`), what it shows folded; empty
    /// when its first member is itself a container.
    pub preview: String,
    /// The preview holds every member.
    pub whole: bool,
}

impl JsonLine {
    /// What a folded container shows: its first members, then how many it holds when they are not all there
    /// (`"id": 12, "status": "paid", … 5 keys`), or the count alone (`… 3 items`).
    pub fn folded(&self) -> String {
        let (one, many) = match self.tok {
            Tok::Open('{') => ("key", "keys"),
            _ => ("item", "items"),
        };
        let count = format!(
            "… {} {}",
            self.count,
            if self.count == 1 { one } else { many }
        );
        match (self.preview.is_empty(), self.whole) {
            (true, _) => count,
            (false, true) => self.preview.clone(),
            (false, false) => format!("{}, {count}", self.preview),
        }
    }

    pub fn opens(&self) -> bool {
        matches!(self.tok, Tok::Open(_))
    }
}

enum J {
    Obj(Vec<(String, J)>),
    Arr(Vec<J>),
    Str(String),
    Num(String),
    Lit(String),
}

struct P<'a> {
    s: &'a [u8],
    at: usize,
    depth: usize,
}

/// Deeper than this, a value is not laid out (it is shown as text).
const DEEPEST: usize = 256;

impl P<'_> {
    fn ws(&mut self) {
        while self.s.get(self.at).is_some_and(u8::is_ascii_whitespace) {
            self.at += 1;
        }
    }

    fn eat(&mut self, c: u8) -> Option<()> {
        self.ws();
        (self.s.get(self.at) == Some(&c)).then(|| self.at += 1)
    }

    fn text(&self, from: usize) -> Option<String> {
        std::str::from_utf8(self.s.get(from..self.at)?)
            .ok()
            .map(str::to_owned)
    }

    fn string(&mut self) -> Option<String> {
        self.ws();
        let from = self.at;
        if self.s.get(self.at) != Some(&b'"') {
            return None;
        }
        self.at += 1;
        loop {
            match self.s.get(self.at)? {
                b'"' => break,
                b'\\' => self.at += 2,
                c if *c < 0x20 => return None,
                _ => self.at += 1,
            }
        }
        self.at += 1;
        self.text(from)
    }

    fn value(&mut self) -> Option<J> {
        self.ws();
        match self.s.get(self.at)? {
            b'{' | b'[' if self.depth >= DEEPEST => None,
            b'{' => {
                self.at += 1;
                self.depth += 1;
                let mut kv = Vec::new();
                if self.eat(b'}').is_none() {
                    loop {
                        let k = self.string()?;
                        self.eat(b':')?;
                        kv.push((k, self.value()?));
                        if self.eat(b',').is_none() {
                            self.eat(b'}')?;
                            break;
                        }
                    }
                }
                self.depth -= 1;
                Some(J::Obj(kv))
            }
            b'[' => {
                self.at += 1;
                self.depth += 1;
                let mut items = Vec::new();
                if self.eat(b']').is_none() {
                    loop {
                        items.push(self.value()?);
                        if self.eat(b',').is_none() {
                            self.eat(b']')?;
                            break;
                        }
                    }
                }
                self.depth -= 1;
                Some(J::Arr(items))
            }
            b'"' => self.string().map(J::Str),
            b't' | b'f' | b'n' => {
                let from = self.at;
                while self.s.get(self.at).is_some_and(u8::is_ascii_lowercase) {
                    self.at += 1;
                }
                let w = self.text(from)?;
                matches!(w.as_str(), "true" | "false" | "null").then_some(J::Lit(w))
            }
            b'-' | b'0'..=b'9' => {
                let from = self.at;
                while self
                    .s
                    .get(self.at)
                    .is_some_and(|c| c.is_ascii_digit() || b"+-.eE".contains(c))
                {
                    self.at += 1;
                }
                let n = self.text(from)?;
                n.parse::<f64>().ok().map(|_| J::Num(n))
            }
            _ => None,
        }
    }
}

/// The value as pretty lines; `None` when it is not (all) JSON.
pub fn lines(body: &[u8]) -> Option<Vec<JsonLine>> {
    let mut p = P {
        s: body,
        at: 0,
        depth: 0,
    };
    let v = p.value()?;
    p.ws();
    if p.at != body.len() {
        return None;
    }
    let mut out = Vec::new();
    lay(&v, None, 0, false, &mut out);
    Some(out)
}

fn lay(v: &J, key: Option<&str>, depth: usize, comma: bool, out: &mut Vec<JsonLine>) {
    let line = |tok| JsonLine {
        depth,
        key: key.map(str::to_owned),
        tok,
        comma,
        end: 0,
        count: 0,
        preview: String::new(),
        whole: false,
    };
    let (open, close, count) = match v {
        J::Obj(kv) if kv.is_empty() => return out.push(line(Tok::Empty("{}"))),
        J::Arr(a) if a.is_empty() => return out.push(line(Tok::Empty("[]"))),
        J::Obj(kv) => ('{', '}', kv.len()),
        J::Arr(a) => ('[', ']', a.len()),
        J::Str(s) => return out.push(line(Tok::Str(s.clone()))),
        J::Num(n) => return out.push(line(Tok::Num(n.clone()))),
        J::Lit(l) => return out.push(line(Tok::Lit(l.clone()))),
    };
    let at = out.len();
    // The comma is the closing line's; a folded container shows it after its summary.
    let (preview, whole) = preview(v);
    out.push(JsonLine {
        count,
        comma: false,
        preview,
        whole,
        ..line(Tok::Open(open))
    });
    match v {
        J::Obj(kv) => {
            for (i, (k, c)) in kv.iter().enumerate() {
                lay(c, Some(k), depth + 1, i + 1 < kv.len(), out);
            }
        }
        J::Arr(a) => {
            for (i, c) in a.iter().enumerate() {
                lay(c, None, depth + 1, i + 1 < a.len(), out);
            }
        }
        _ => {}
    }
    let end = out.len();
    out.push(JsonLine {
        depth,
        key: None,
        tok: Tok::Close(close),
        comma,
        end: 0,
        count: 0,
        preview: String::new(),
        whole: false,
    });
    if let Some(l) = out.get_mut(at) {
        l.end = end;
    }
}

/// How long a folded container's preview runs.
const PREVIEW: usize = 64;

/// A container's first scalar members in a line, and whether that is all of them. It stops at a member that is a
/// container, and before the line runs past [`PREVIEW`].
fn preview(v: &J) -> (String, bool) {
    let scalar = |j: &J| match j {
        J::Str(s) | J::Num(s) | J::Lit(s) => Some(s.clone()),
        J::Obj(kv) if kv.is_empty() => Some("{}".to_owned()),
        J::Arr(a) if a.is_empty() => Some("[]".to_owned()),
        _ => None,
    };
    let parts: Vec<Option<String>> = match v {
        J::Obj(kv) => kv
            .iter()
            .map(|(k, c)| scalar(c).map(|s| format!("{k}: {s}")))
            .collect(),
        J::Arr(a) => a.iter().map(scalar).collect(),
        _ => return (String::new(), false),
    };
    let mut out = String::new();
    let mut taken = 0;
    for p in &parts {
        let Some(p) = p else { break };
        if !out.is_empty() && out.chars().count() + p.chars().count() + 2 > PREVIEW {
            break;
        }
        if out.is_empty() && p.chars().count() > PREVIEW {
            break;
        }
        if !out.is_empty() {
            out.push_str(", ");
        }
        out.push_str(p);
        taken += 1;
    }
    (out, taken == parts.len())
}

/// The lines shown with `folded` closed: each line's index, and whether it stands for its folded container.
pub fn visible(lines: &[JsonLine], folded: &HashSet<usize>) -> Vec<(usize, bool)> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(l) = lines.get(i) {
        if l.opens() && folded.contains(&i) {
            out.push((i, true));
            i = l.end + 1;
        } else {
            out.push((i, false));
            i += 1;
        }
    }
    out
}

/// The containers folded when a value opens: none in a short value; in a long one every container below the top
/// level, so its keys read at a glance and each opens with a click.
pub fn first_folds(lines: &[JsonLine]) -> HashSet<usize> {
    if lines.len() <= 40 {
        return HashSet::new();
    }
    lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.opens() && l.depth >= 1)
        .map(|(i, _)| i)
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn text(lines: &[JsonLine], folded: &HashSet<usize>) -> String {
        visible(lines, folded)
            .into_iter()
            .map(|(i, f)| {
                let l = &lines[i];
                let key = l
                    .key
                    .as_deref()
                    .map(|k| format!("{k}: "))
                    .unwrap_or_default();
                let tok = match &l.tok {
                    Tok::Open(c) if f => format!("{c} {} {}", l.folded(), lines[l.end].tok_str()),
                    t => t_str(t),
                };
                let comma = if l.comma || (f && lines[l.end].comma) {
                    ","
                } else {
                    ""
                };
                format!("{}{key}{tok}{comma}", "  ".repeat(l.depth))
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn t_str(t: &Tok) -> String {
        match t {
            Tok::Open(c) | Tok::Close(c) => c.to_string(),
            Tok::Empty(s) => (*s).to_owned(),
            Tok::Str(s) | Tok::Num(s) | Tok::Lit(s) => s.clone(),
        }
    }

    impl JsonLine {
        fn tok_str(&self) -> String {
            t_str(&self.tok)
        }
    }

    #[test]
    fn pretty_keeps_order_and_digits() {
        let l =
            lines(br#"{"z":1.50,"a":[12345678901234567890,true,null],"e":{},"s":"x\"y"}"#).unwrap();
        assert_eq!(
            text(&l, &HashSet::new()),
            "{\n  \"z\": 1.50,\n  \"a\": [\n    12345678901234567890,\n    true,\n    null\n  ],\n  \"e\": {},\n  \"s\": \"x\\\"y\"\n}"
        );
    }

    #[test]
    fn a_folded_container_says_what_it_holds() {
        let l = lines(br#"{"lines":[{"a":1},{"a":2},{"a":3}],"c":{"id":1,"n":"x"}}"#).unwrap();
        let folded: HashSet<usize> = [1, 12].into_iter().collect();
        assert_eq!(
            text(&l, &folded),
            "{\n  \"lines\": [ … 3 items ],\n  \"c\": { \"id\": 1, \"n\": \"x\" }\n}"
        );
    }

    #[test]
    fn not_json_is_none() {
        for bad in [&b"{\"a\":"[..], b"[1,]", b"nul", b"{} x", b"\"\x01\""] {
            assert!(lines(bad).is_none(), "{:?}", String::from_utf8_lossy(bad));
        }
        assert!(lines(&[b'['; 300]).is_none(), "too deep");
    }

    #[test]
    fn a_long_value_opens_with_its_containers_folded() {
        let long = format!("[{}]", vec!["{\"a\":1}"; 30].join(","));
        let l = lines(long.as_bytes()).unwrap();
        let f = first_folds(&l);
        assert_eq!(f.len(), 30, "every item, not the top");
        assert!(first_folds(&lines(b"{\"a\":{\"b\":1}}").unwrap()).is_empty());
    }
}
