//! The template language API Gateway's mapping templates are written in (Velocity), read into a tree. The subset: text, references
//! (`$x`, `$!x`, `${x}`, properties, indexes and method calls), `#set`, `#if`/`#elseif`/`#else`, `#foreach`, `##` and `#* *#`
//! comments, and expressions with literals, lists, maps, ranges and the usual operators. `#macro`, `#parse`, `#include`, `#define`,
//! `#evaluate`, `#break` and `#stop` are refused by name: a template that uses them is not run at all, rather than run wrongly.
//!
//! Nesting is capped ([`MAX_DEPTH`]) and so is the template's size, so a hostile template cannot overflow the stack.

use super::{Access, Expr, Node, Ref};

/// How deep directives and expressions may nest.
pub(crate) const MAX_DEPTH: usize = 64;
/// The largest template read (API Gateway's own limit is 300 KB).
pub(crate) const MAX_LEN: usize = 300 * 1024;

/// Why a template cannot be run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Error(pub(crate) String);

pub(super) type R<T> = Result<T, Error>;

pub(super) fn err<T>(why: impl Into<String>) -> R<T> {
    Err(Error(why.into()))
}

/// The reader: the template, the cursor, and how deep it is.
pub(super) struct P<'a> {
    pub(super) s: &'a str,
    pub(super) i: usize,
    pub(super) depth: usize,
}

// fuzz:parser fuzz_vtl
/// Reads a template.
///
/// # Errors
/// A template that is too large, nests too deep, is malformed, or uses a directive that is not built.
pub(crate) fn parse(s: &str) -> R<Vec<Node>> {
    if s.len() > MAX_LEN {
        return err("the mapping template is larger than 300 KB");
    }
    let mut p = P { s, i: 0, depth: 0 };
    let (nodes, end) = p.block()?;
    match end {
        End::Eof => Ok(nodes),
        End::Else | End::ElseIf(_) | End::Stop => {
            err("#else, #elseif or #end without #if or #foreach")
        }
    }
}

/// What ended a block.
pub(super) enum End {
    Eof,
    Stop,
    Else,
    ElseIf(Expr),
}

impl P<'_> {
    pub(super) fn rest(&self) -> &str {
        self.s.get(self.i..).unwrap_or_default()
    }

    pub(super) fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    pub(super) fn eat(&mut self, lit: &str) -> bool {
        if self.rest().starts_with(lit) {
            self.i += lit.len();
            true
        } else {
            false
        }
    }

    pub(super) fn deeper(&mut self) -> R<()> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return err("the mapping template nests too deep");
        }
        Ok(())
    }

    pub(super) fn block(&mut self) -> R<(Vec<Node>, End)> {
        self.deeper()?;
        let mut nodes = Vec::new();
        let mut text = String::new();
        let flush = |text: &mut String, nodes: &mut Vec<Node>| {
            if !text.is_empty() {
                nodes.push(Node::Text(std::mem::take(text)));
            }
        };
        loop {
            let Some(c) = self.peek() else {
                flush(&mut text, &mut nodes);
                self.depth -= 1;
                return Ok((nodes, End::Eof));
            };
            if c == '#' {
                if let Some(end) = self.directive(&mut nodes, &mut text)? {
                    flush(&mut text, &mut nodes);
                    self.depth -= 1;
                    return Ok((nodes, end));
                }
                continue;
            }
            if c == '$'
                && let Some((r, quiet)) = self.reference()?
            {
                flush(&mut text, &mut nodes);
                nodes.push(Node::Ref(r, quiet));
                continue;
            }
            if c == '\\' && self.rest().get(1..2) == Some("$") {
                self.i += 2;
                text.push('$');
                continue;
            }
            text.push(c);
            self.i += c.len_utf8();
        }
    }

    /// A `#` at the cursor: a directive, a comment, or plain text. `Some` when it ends the block.
    fn directive(&mut self, nodes: &mut Vec<Node>, text: &mut String) -> R<Option<End>> {
        if self.eat("##") {
            let n = self.rest().find('\n').unwrap_or(self.rest().len());
            self.i += n;
            return Ok(None);
        }
        if self.eat("#*") {
            let n = self.rest().find("*#").map_or(self.rest().len(), |n| n + 2);
            self.i += n;
            return Ok(None);
        }
        let start = self.i;
        self.i += 1;
        let braced = self.eat("{");
        let name: String = self
            .rest()
            .chars()
            .take_while(char::is_ascii_alphabetic)
            .collect();
        self.i += name.len();
        if braced && !self.eat("}") {
            self.i = start + 1;
            text.push('#');
            return Ok(None);
        }
        let done = |n: Node, nodes: &mut Vec<Node>, text: &mut String| {
            if !text.is_empty() {
                nodes.push(Node::Text(std::mem::take(text)));
            }
            nodes.push(n);
        };
        match name.as_str() {
            "set" => {
                let n = self.set()?;
                done(n, nodes, text);
            }
            "if" => {
                let n = self.if_chain()?;
                done(n, nodes, text);
            }
            "foreach" => {
                let n = self.foreach()?;
                done(n, nodes, text);
            }
            "end" => return Ok(Some(End::Stop)),
            "else" => return Ok(Some(End::Else)),
            "elseif" => {
                let cond = self.paren_expr()?;
                return Ok(Some(End::ElseIf(cond)));
            }
            "macro" | "parse" | "include" | "define" | "evaluate" | "break" | "stop" => {
                return err(format!("the #{name} directive is not built in doze-aws"));
            }
            _ => {
                self.i = start + 1;
                text.push('#');
            }
        }
        Ok(None)
    }

    pub(super) fn ws(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.i += 1;
        }
    }

    pub(super) fn paren_expr(&mut self) -> R<Expr> {
        self.ws();
        if !self.eat("(") {
            return err("a directive's ( is missing");
        }
        let e = self.expr()?;
        self.ws();
        if !self.eat(")") {
            return err("a directive's ) is missing");
        }
        Ok(e)
    }

    fn set(&mut self) -> R<Node> {
        self.ws();
        if !self.eat("(") {
            return err("#set needs (");
        }
        self.ws();
        let Some((target, _)) = self.reference()? else {
            return err("#set needs a reference to set");
        };
        self.ws();
        if !self.eat("=") {
            return err("#set needs =");
        }
        let value = self.expr()?;
        self.ws();
        if !self.eat(")") {
            return err("#set needs )");
        }
        Ok(Node::Set(target, value))
    }

    fn if_chain(&mut self) -> R<Node> {
        let mut arms = Vec::new();
        let mut cond = self.paren_expr()?;
        loop {
            let (body, end) = self.block()?;
            arms.push((cond, body));
            match end {
                End::Stop => return Ok(Node::If(arms, None)),
                End::ElseIf(next) => cond = next,
                End::Else => {
                    let (body, end) = self.block()?;
                    return match end {
                        End::Stop => Ok(Node::If(arms, Some(body))),
                        _ => err("#else must end with #end"),
                    };
                }
                End::Eof => return err("#if without #end"),
            }
        }
    }

    fn foreach(&mut self) -> R<Node> {
        self.ws();
        if !self.eat("(") {
            return err("#foreach needs (");
        }
        self.ws();
        let Some((var, _)) = self.reference()? else {
            return err("#foreach needs a variable");
        };
        self.ws();
        if !self.eat("in") {
            return err("#foreach needs in");
        }
        let over = self.expr()?;
        self.ws();
        if !self.eat(")") {
            return err("#foreach needs )");
        }
        let (body, end) = self.block()?;
        match end {
            End::Stop => Ok(Node::Foreach(var.root, over, body)),
            _ => err("#foreach without #end"),
        }
    }

    fn ident(&mut self) -> String {
        let id: String = self
            .rest()
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
            .collect();
        let id = id.trim_end_matches('-').to_owned();
        self.i += id.len();
        id
    }

    /// A reference at `$`, or `None` (the cursor unmoved) when the `$` is plain text.
    pub(super) fn reference(&mut self) -> R<Option<(Ref, bool)>> {
        let start = self.i;
        if !self.eat("$") {
            return Ok(None);
        }
        let quiet = self.eat("!");
        let braced = self.eat("{");
        if !self
            .peek()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        {
            self.i = start;
            return Ok(None);
        }
        let root = self.ident();
        let mut path = Vec::new();
        loop {
            let save = self.i;
            if self.eat(".") {
                if !self
                    .peek()
                    .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                {
                    self.i = save;
                    break;
                }
                let name = self.ident();
                if self.eat("(") {
                    let args = self.args(')')?;
                    path.push(Access::Call(name, args));
                } else {
                    path.push(Access::Prop(name));
                }
            } else if self.eat("[") {
                let e = self.expr()?;
                self.ws();
                if !self.eat("]") {
                    return err("an index's ] is missing");
                }
                path.push(Access::Index(e));
            } else {
                break;
            }
        }
        if braced && !self.eat("}") {
            return err("a reference's } is missing");
        }
        let text = self.s.get(start..self.i).unwrap_or_default().to_owned();
        Ok(Some((Ref { root, path, text }, quiet)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directives_references_and_comments_are_read() {
        let t = parse("a$x.y(1, 'z')b#set($n = 1 + 2)#if($n > 2)big#elseif($n == 1)one#{else}small#end## gone\n#* also *#c").unwrap();
        assert_eq!(t.len(), 6);
        assert!(matches!(&t[1], Node::Ref(r, false) if r.root == "x" && r.path.len() == 1));
        assert!(matches!(&t[4], Node::If(arms, Some(_)) if arms.len() == 2));
        assert_eq!(t[5], Node::Text("\nc".into()));
    }

    #[test]
    fn what_is_not_built_or_broken_is_refused() {
        assert!(parse("#macro(x)#end").unwrap_err().0.contains("#macro"));
        assert!(parse("#if($x)").is_err());
        assert!(parse("#end").is_err());
        assert!(parse(&"#if(true)".repeat(100)).is_err());
        assert!(parse(&"(".repeat(200)).is_ok());
        assert!(parse(&format!("#set($x = {})", "(".repeat(200))).is_err());
    }

    #[test]
    fn a_dollar_that_names_nothing_is_text() {
        assert_eq!(
            parse("cost: $5").unwrap(),
            vec![Node::Text("cost: $5".into())]
        );
    }
}
