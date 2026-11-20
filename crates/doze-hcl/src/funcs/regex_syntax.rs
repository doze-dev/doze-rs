//! Go's regular expressions (RE2 as `regexp/syntax` parses them with the `Perl` flags) translated
//! to the `regex` crate's syntax. Go's parser decides what is valid, so this one follows it: the
//! same escapes, classes, flags and repetition rules and the same errors; then every construct is
//! written in a form the crate reads the same way. Where the two differ in meaning it is spelled
//! out: `\d \s \w \b` are ASCII in Go, a `{` that starts no repetition is a literal, `&&` and `[`
//! inside a class are plain characters, and named groups are written unnamed (the names are kept
//! here, in group order, so Go's names need not satisfy the crate's rules).

use super::regex_class::{self, braces};

/// Groups nest at most this deep (Go allows a parse tree 1,000 high; the parser here recurses, and
/// the `regex` crate's own default is 250).
const MAX_DEPTH: usize = 250;

/// A translated pattern and the name (if any) of each capture group, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Translated {
    pub(super) pattern: String,
    pub(super) names: Vec<Option<String>>,
}

#[derive(Debug, Clone)]
enum Node {
    /// Already in the crate's syntax: a literal, class, assertion or `.`.
    Atom(String),
    /// `(?flags)`: changes the flags for the rest of the group; repeats nothing.
    Flags(String),
    Concat(Vec<Node>),
    Alt(Vec<Node>),
    Group {
        cap: bool,
        flags: String,
        sub: Box<Node>,
    },
    Repeat {
        sub: Box<Node>,
        op: String,
        /// For `{min,max}` (Go's `OpRepeat`): min and max, -1 for no max.
        counted: Option<(u32, i64)>,
    },
}

/// A repetition operator in the crate's syntax, with its counts if it is `{min,max}`.
type Repeat = (String, Option<(u32, i64)>);

/// A Go `syntax.Error`: its code and the part of the pattern it is about.
pub(super) fn err(code: &str, expr: &str) -> String {
    format!("invalid regexp pattern: {code} in {expr}")
}

pub(super) fn translate(pattern: &str) -> Result<Translated, String> {
    let mut p = Parser {
        s: pattern,
        at: 0,
        names: Vec::new(),
    };
    let node = p.alt(0)?;
    if p.at < p.s.len() {
        return Err(err("unexpected )", pattern));
    }
    let mut out = String::new();
    emit(&node, &mut out);
    Ok(Translated {
        pattern: out,
        names: p.names,
    })
}

pub(super) struct Parser<'a> {
    s: &'a str,
    at: usize,
    names: Vec<Option<String>>,
}

impl Parser<'_> {
    pub(super) fn rest(&self) -> &str {
        self.s.get(self.at..).unwrap_or("")
    }

    pub(super) fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    pub(super) fn advance(&mut self, n: usize) {
        self.at += n;
    }

    fn since(&self, start: usize) -> &str {
        self.s.get(start..self.at).unwrap_or("")
    }

    fn alt(&mut self, depth: usize) -> Result<Node, String> {
        if depth >= MAX_DEPTH {
            return Err(err("expression nests too deeply", self.s));
        }
        let mut branches = vec![self.concat(depth)?];
        while self.peek() == Some('|') {
            self.at += 1;
            branches.push(self.concat(depth)?);
        }
        Ok(match branches.len() {
            1 => branches.pop().unwrap_or(Node::Concat(Vec::new())),
            _ => Node::Alt(branches),
        })
    }

    fn concat(&mut self, depth: usize) -> Result<Node, String> {
        let mut items: Vec<Node> = Vec::new();
        let mut last_repeat = false;
        while let Some(c) = self.peek() {
            let start = self.at;
            let mut repeated = false;
            match c {
                '|' | ')' => break,
                '(' => items.push(self.group(depth)?),
                '[' => items.push(Node::Atom(regex_class::class(self)?)),
                '*' | '+' | '?' | '{' => match self.repeat_op()? {
                    None => items.push(Node::Atom(literal('{'))),
                    Some(_) if last_repeat => {
                        return Err(err("invalid nested repetition operator", self.since(start)));
                    }
                    Some((op, counted)) => {
                        apply_repeat(&mut items, op, counted, self.since(start))?;
                        repeated = true;
                    }
                },
                '\\' => self.escape(&mut items)?,
                '.' | '^' | '$' => {
                    self.at += 1;
                    items.push(Node::Atom(c.to_string()));
                }
                c => {
                    self.at += c.len_utf8();
                    items.push(Node::Atom(literal(c)));
                }
            }
            last_repeat = repeated;
        }
        Ok(Node::Concat(items))
    }

    /// `*`, `+`, `?` or a valid `{n}`, `{n,}`, `{n,m}`, with an optional `?`; `None` for a `{` that
    /// starts no repetition (it is a literal and has been consumed).
    fn repeat_op(&mut self) -> Result<Option<Repeat>, String> {
        let start = self.at;
        let (mut op, counted) = if self.peek() == Some('{') {
            let Some(b) = braces(self.rest()) else {
                self.at += 1;
                return Ok(None);
            };
            self.at += b.len;
            let Some((min, max)) = b.counts() else {
                return Err(err("invalid repeat count", self.since(start)));
            };
            let op = match max {
                -1 => format!("{{{min},}}"),
                m if b.single => format!("{{{m}}}"),
                m => format!("{{{min},{m}}}"),
            };
            (op, Some((min, max)))
        } else {
            self.at += 1;
            (self.since(start).to_owned(), None)
        };
        if self.peek() == Some('?') {
            self.at += 1;
            op.push('?');
        }
        Ok(Some((op, counted)))
    }

    fn group(&mut self, depth: usize) -> Result<Node, String> {
        let start = self.at;
        self.at += 1;
        let (cap, flags) = match self.rest().strip_prefix('?') {
            None => {
                self.names.push(None);
                (true, String::new())
            }
            Some(after) => match after.strip_prefix("P<").or_else(|| after.strip_prefix('<')) {
                Some(name_on) => {
                    let head = self.s.len() - name_on.len();
                    self.named(start, head)?;
                    (true, String::new())
                }
                None => match self.flags(start)? {
                    (f, true) => return Ok(Node::Flags(f)),
                    (f, false) => (false, f),
                },
            },
        };
        let sub = self.alt(depth + 1)?;
        if self.peek() != Some(')') {
            return Err(err("missing closing )", self.s));
        }
        self.at += 1;
        Ok(Node::Group {
            cap,
            flags,
            sub: Box::new(sub),
        })
    }

    /// `(?P<name>` or `(?<name>`; the name starts at `head`.
    fn named(&mut self, start: usize, head: usize) -> Result<(), String> {
        let rest = self.s.get(head..).unwrap_or("");
        let Some(end) = rest.find('>') else {
            return Err(err(
                "invalid named capture",
                self.s.get(start..).unwrap_or(""),
            ));
        };
        let name = rest.get(..end).unwrap_or("");
        self.at = head + end + 1;
        if name.is_empty() || !name.chars().all(|c| c == '_' || c.is_ascii_alphanumeric()) {
            return Err(err("invalid named capture", self.since(start)));
        }
        self.names.push(Some(name.to_owned()));
        Ok(())
    }

    /// `(?flags)` or `(?flags:` after the `(`: Go's `parsePerlFlags`. Returns the flags in the
    /// crate's syntax and whether the group closed (`)`) rather than opened (`:`).
    fn flags(&mut self, start: usize) -> Result<(String, bool), String> {
        self.at += 1;
        let (mut on, mut off) = (String::new(), String::new());
        let (mut negated, mut saw) = (false, false);
        while let Some(c) = self.peek() {
            self.at += c.len_utf8();
            match c {
                'i' | 'm' | 's' | 'U' => {
                    let set = if negated { &mut off } else { &mut on };
                    if !set.contains(c) {
                        set.push(c);
                    }
                    saw = true;
                }
                '-' if !negated => (negated, saw) = (true, false),
                ':' | ')' if !negated || saw => {
                    on.retain(|f| !off.contains(f));
                    let text = match (on.is_empty(), off.is_empty()) {
                        (true, true) => String::new(),
                        (_, true) => on,
                        _ => format!("{on}-{off}"),
                    };
                    return Ok((text, c == ')'));
                }
                _ => break,
            }
        }
        Err(err("invalid or unsupported Perl syntax", self.since(start)))
    }

    fn escape(&mut self, items: &mut Vec<Node>) -> Result<(), String> {
        let atom = match self.rest().get(1..2) {
            Some("A") => "\\A",
            Some("z") => "\\z",
            Some("b") => "(?-u:\\b)",
            Some("B") => "(?-u:\\B)",
            Some("C") => return Err(err("invalid escape sequence", "\\C")),
            Some("Q") => {
                let body = self.rest().get(2..).unwrap_or("");
                let (lit, used) = match body.find("\\E") {
                    Some(i) => (body.get(..i).unwrap_or(""), i + 4),
                    None => (body, body.len() + 2),
                };
                items.extend(lit.chars().map(|c| Node::Atom(literal(c))));
                self.at += used;
                return Ok(());
            }
            _ => "",
        };
        if !atom.is_empty() {
            self.at += 2;
            items.push(Node::Atom(atom.to_owned()));
            return Ok(());
        }
        let text = match regex_class::escape_class(self)? {
            Some(class) => class,
            None => literal_code(regex_class::escape_char(self)?),
        };
        items.push(Node::Atom(text));
        Ok(())
    }
}

/// Applies a repetition to the last item that is not a flags change (Go repeats the top of its
/// stack, which a flags change does not touch).
fn apply_repeat(
    items: &mut [Node],
    op: String,
    counted: Option<(u32, i64)>,
    text: &str,
) -> Result<(), String> {
    let Some(last) = items
        .iter_mut()
        .rev()
        .find(|n| !matches!(n, Node::Flags(_)))
    else {
        return Err(err("missing argument to repetition operator", text));
    };
    let sub = std::mem::replace(last, Node::Concat(Vec::new()));
    let node = Node::Repeat {
        sub: Box::new(sub),
        op,
        counted,
    };
    let checked = counted.is_some_and(|(min, max)| min >= 2 || max >= 2);
    if checked && !repeat_is_valid(&node, 1000) {
        return Err(err("invalid repeat count", text));
    }
    *last = node;
    Ok(())
}

/// Go's `repeatIsValid`: nested counted repetitions may not multiply past `n`.
fn repeat_is_valid(node: &Node, mut n: i64) -> bool {
    match node {
        Node::Repeat { sub, counted, .. } => {
            if let Some((min, max)) = *counted {
                if max == 0 {
                    return true;
                }
                let m = if max < 0 { i64::from(min) } else { max };
                if m > n {
                    return false;
                }
                if m > 0 {
                    n /= m;
                }
            }
            repeat_is_valid(sub, n)
        }
        Node::Group { sub, .. } => repeat_is_valid(sub, n),
        Node::Concat(items) | Node::Alt(items) => items.iter().all(|i| repeat_is_valid(i, n)),
        Node::Atom(_) | Node::Flags(_) => true,
    }
}

fn emit(node: &Node, out: &mut String) {
    match node {
        Node::Atom(a) => out.push_str(a),
        Node::Flags(f) if f.is_empty() => {}
        Node::Flags(f) => {
            out.push_str("(?");
            out.push_str(f);
            out.push(')');
        }
        Node::Concat(items) => items.iter().for_each(|i| emit(i, out)),
        Node::Alt(items) => {
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push('|');
                }
                emit(item, out);
            }
        }
        Node::Group { cap: true, sub, .. } => {
            out.push('(');
            emit(sub, out);
            out.push(')');
        }
        Node::Group { flags, sub, .. } => {
            out.push_str("(?");
            out.push_str(flags);
            out.push(':');
            emit(sub, out);
            out.push(')');
        }
        Node::Repeat { sub, op, .. } => {
            out.push_str("(?:");
            emit(sub, out);
            out.push(')');
            out.push_str(op);
        }
    }
}

/// A literal character in the crate's syntax.
pub(super) fn literal(c: char) -> String {
    if c.is_alphanumeric() {
        c.to_string()
    } else {
        format!("\\x{{{:X}}}", u32::from(c))
    }
}

/// A literal from an escape, which in Go may name a surrogate (that never matches text).
pub(super) fn literal_code(code: u32) -> String {
    match char::from_u32(code) {
        Some(c) => literal(c),
        None => "[^\\x{0}-\\x{10FFFF}]".to_owned(),
    }
}
