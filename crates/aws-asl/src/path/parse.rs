//! Reading a path's text into steps.

use super::{Path, Root, Seg};

/// A cursor over the text of a path or a filter.
pub(super) struct Cursor<'a> {
    pub(super) text: &'a str,
    pub(super) at: usize,
}

impl<'a> Cursor<'a> {
    pub(super) const fn new(text: &'a str) -> Self {
        Self { text, at: 0 }
    }

    pub(super) fn rest(&self) -> &'a str {
        self.text.get(self.at..).unwrap_or_default()
    }

    pub(super) fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    pub(super) fn eat(&mut self, s: &str) -> bool {
        if self.rest().starts_with(s) {
            self.at += s.len();
            true
        } else {
            false
        }
    }

    pub(super) fn blank(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.at += 1;
        }
    }

    pub(super) fn done(&self) -> bool {
        self.at >= self.text.len()
    }

    /// A name after a dot: up to the next `.`, `[` or the end (or, inside a filter, an operator or a bracket).
    pub(super) fn name(&mut self, in_filter: bool) -> &'a str {
        let rest = self.rest();
        let end = rest
            .char_indices()
            .find(|(_, c)| {
                matches!(c, '.' | '[')
                    || (in_filter
                        && (c.is_whitespace()
                            || matches!(c, ')' | '=' | '!' | '<' | '>' | '&' | '|' | ']' | ',')))
            })
            .map_or(rest.len(), |(i, _)| i);
        self.at += end;
        rest.get(..end).unwrap_or_default()
    }

    /// A quoted string, `'...'` or `"..."`, with backslash escapes.
    pub(super) fn quoted(&mut self) -> Result<String, String> {
        let Some(q) = self.peek().filter(|c| *c == '\'' || *c == '"') else {
            return Err("a quoted name was expected".to_owned());
        };
        self.at += 1;
        let mut out = String::new();
        let mut chars = self.rest().char_indices();
        while let Some((i, c)) = chars.next() {
            if c == q {
                self.at += i + 1;
                return Ok(out);
            }
            if c == '\\' {
                if let Some((_, e)) = chars.next() {
                    out.push(e);
                }
                continue;
            }
            out.push(c);
        }
        Err("a quoted name is not closed".to_owned())
    }

    /// A whole number, perhaps negative.
    pub(super) fn int(&mut self) -> Option<i64> {
        self.blank();
        let rest = self.rest();
        let len = rest
            .char_indices()
            .take_while(|(i, c)| c.is_ascii_digit() || (*i == 0 && *c == '-'))
            .count();
        let n = rest.get(..len)?.parse().ok()?;
        self.at += len;
        self.blank();
        Some(n)
    }
}

// fuzz:parser fuzz_jsonpath
pub(super) fn path(text: &str) -> Result<Path, String> {
    let mut c = Cursor::new(text);
    let root = root(&mut c, text)?;
    let segs = steps(&mut c, false)?;
    if !c.done() {
        return Err(format!("unexpected text at position {} of {text}", c.at));
    }
    Ok(Path {
        root,
        segs,
        text: text.to_owned(),
    })
}

fn root(c: &mut Cursor<'_>, text: &str) -> Result<Root, String> {
    if c.eat("$$") {
        return Ok(Root::Context);
    }
    if !c.eat("$") {
        return Err(format!("a path must start with $: {text}"));
    }
    if c.peek().is_some_and(|ch| ch.is_alphabetic() || ch == '_') {
        let name = c.name(false);
        return Ok(Root::Var(name.to_owned()));
    }
    Ok(Root::Input)
}

/// The steps after a root, until the end (or, in a filter, the first character that is not a step).
pub(super) fn steps(c: &mut Cursor<'_>, in_filter: bool) -> Result<Vec<Seg>, String> {
    let mut segs = Vec::new();
    loop {
        if c.eat("..") {
            let step = if c.peek() == Some('[') {
                bracket(c)?
            } else {
                dotted(c, in_filter)?
            };
            segs.push(Seg::Descend(Box::new(step)));
        } else if c.eat(".") {
            segs.push(dotted(c, in_filter)?);
        } else if c.peek() == Some('[') {
            segs.push(bracket(c)?);
        } else {
            return Ok(segs);
        }
    }
}

fn dotted(c: &mut Cursor<'_>, in_filter: bool) -> Result<Seg, String> {
    if c.eat("*") {
        return Ok(Seg::Wild);
    }
    let name = c.name(in_filter);
    if name.is_empty() {
        return Err("a field name is empty".to_owned());
    }
    if name == "length()" {
        return Ok(Seg::Length);
    }
    if c.eat("()") {
        return Err(format!("the function {name}() is not supported"));
    }
    Ok(Seg::Field(name.to_owned()))
}

fn bracket(c: &mut Cursor<'_>) -> Result<Seg, String> {
    c.eat("[");
    c.blank();
    let seg = if c.eat("*") {
        Seg::Wild
    } else if c.eat("?") {
        Seg::Filter(Box::new(super::filter::parse(c)?))
    } else if matches!(c.peek(), Some('\'' | '"')) {
        let mut names = vec![c.quoted()?];
        c.blank();
        while c.eat(",") {
            c.blank();
            names.push(c.quoted()?);
            c.blank();
        }
        if names.len() == 1 {
            Seg::Field(names.remove(0))
        } else {
            Seg::Fields(names)
        }
    } else {
        numbers(c)?
    };
    c.blank();
    if !c.eat("]") {
        return Err("a [ is not closed".to_owned());
    }
    Ok(seg)
}

fn numbers(c: &mut Cursor<'_>) -> Result<Seg, String> {
    let first = c.int();
    if c.eat(":") {
        let end = c.int();
        let step = if c.eat(":") { c.int() } else { None };
        return Ok(Seg::Slice(first, end, step));
    }
    let first = first.ok_or_else(|| "an index or a quoted name was expected".to_owned())?;
    let mut all = vec![first];
    while c.eat(",") {
        all.push(c.int().ok_or_else(|| "an index was expected".to_owned())?);
    }
    Ok(if all.len() == 1 {
        Seg::Index(first)
    } else {
        Seg::Indices(all)
    })
}

#[cfg(test)]
mod tests {
    use super::super::{Path, Root, Seg};

    #[test]
    fn paths_read() {
        let p = Path::parse("$.a['b c'][0]").unwrap();
        assert_eq!(p.root, Root::Input);
        assert_eq!(
            p.segs,
            vec![
                Seg::Field("a".into()),
                Seg::Field("b c".into()),
                Seg::Index(0)
            ]
        );
        assert!(p.is_reference());
        assert_eq!(
            Path::parse("$$.Map.Item.Value").unwrap().root,
            Root::Context
        );
        assert_eq!(
            Path::parse("$total.x").unwrap().root,
            Root::Var("total".into())
        );
        assert!(!Path::parse("$..a").unwrap().definite());
        assert!(!Path::parse("$.a[*].b").unwrap().definite());
        assert!(!Path::parse("$.a[1:3]").unwrap().definite());
        assert!(Path::parse("$.a[?(@.x > 1)]").is_ok());
        assert!(Path::parse("a.b").is_err());
        assert!(Path::parse("$.a[").is_err());
        assert!(Path::parse("$").unwrap().segs.is_empty());
    }
}
