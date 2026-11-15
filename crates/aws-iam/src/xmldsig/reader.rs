//! The reader of the documents a signature is checked in, small and bounded for this one job: it refuses a document type
//! declaration (the door to entity expansion), stops at the [`Limits`], and applies XML's normalizations so canonical output
//! matches what a signer computed: line ends become LF, and a literal tab or line end in an attribute value becomes a space (a
//! character reference such as `&#xA;` stays the character).

use super::tree::{Attr, Element, Limits, Node, local, prefix};

const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";

/// Reads a document into its document element. The error says, in a few words, what was wrong.
pub(super) fn parse(text: &str, limits: Limits) -> Result<Element, String> {
    if text.len() > limits.bytes {
        return Err(format!("it is longer than {} bytes", limits.bytes));
    }
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let normalized;
    let text = if text.contains('\r') {
        normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        normalized.as_str()
    } else {
        text
    };
    let mut reader = Reader {
        limits,
        open: Vec::new(),
        scope: Vec::new(),
        marks: Vec::new(),
        root: None,
        elements: 0,
    };
    let mut rest = text;
    while let Some(lt) = rest.find('<') {
        reader.text(rest.get(..lt).ok_or(MALFORMED)?)?;
        rest = reader.markup(rest.get(lt + 1..).ok_or(MALFORMED)?)?;
    }
    reader.text(rest)?;
    if !reader.open.is_empty() {
        return Err("an element is not closed".to_owned());
    }
    reader.root.ok_or_else(|| "it has no element".to_owned())
}

const MALFORMED: &str = "it is not well-formed XML";

struct Reader {
    limits: Limits,
    /// The open elements, outermost first.
    open: Vec<Element>,
    /// The namespace declarations in scope: prefix and URI, innermost last.
    scope: Vec<(String, String)>,
    /// Where each open element's declarations start in `scope`.
    marks: Vec<usize>,
    root: Option<Element>,
    elements: usize,
}

impl Reader {
    fn text(&mut self, raw: &str) -> Result<(), String> {
        if raw.is_empty() {
            return Ok(());
        }
        let Some(parent) = self.open.last_mut() else {
            return if raw.trim().is_empty() {
                Ok(())
            } else {
                Err("it has text outside its document element".to_owned())
            };
        };
        let text = unescape(raw).ok_or(MALFORMED)?;
        push_text(parent, &text);
        Ok(())
    }

    /// Reads the markup `rest` starts with (just after its `<`), and gives back what follows it.
    fn markup<'a>(&mut self, rest: &'a str) -> Result<&'a str, String> {
        if let Some(after) = rest.strip_prefix("!--") {
            let end = after.find("--").ok_or(MALFORMED)?;
            return after
                .get(end..)
                .and_then(|s| s.strip_prefix("-->"))
                .ok_or_else(|| MALFORMED.to_owned());
        }
        if let Some(after) = rest.strip_prefix("![CDATA[") {
            let end = after.find("]]>").ok_or(MALFORMED)?;
            let parent = self.open.last_mut().ok_or(MALFORMED)?;
            push_text(parent, after.get(..end).ok_or(MALFORMED)?);
            return after.get(end + 3..).ok_or_else(|| MALFORMED.to_owned());
        }
        if rest.starts_with('!') {
            return Err("it has a document type declaration".to_owned());
        }
        if let Some(after) = rest.strip_prefix('?') {
            let end = after.find("?>").ok_or(MALFORMED)?;
            self.pi(after.get(..end).ok_or(MALFORMED)?);
            return after.get(end + 2..).ok_or_else(|| MALFORMED.to_owned());
        }
        if let Some(after) = rest.strip_prefix('/') {
            let gt = after.find('>').ok_or(MALFORMED)?;
            self.close(after.get(..gt).ok_or(MALFORMED)?.trim_end())?;
            return after.get(gt + 1..).ok_or_else(|| MALFORMED.to_owned());
        }
        self.open_tag(rest)
    }

    /// Keeps a processing instruction inside the document element; the XML declaration and any outside it are dropped.
    fn pi(&mut self, body: &str) {
        let (target, data) = body
            .split_once(|c: char| c.is_ascii_whitespace())
            .unwrap_or((body, ""));
        if let Some(parent) = self.open.last_mut() {
            parent
                .children
                .push(Node::Pi(target.to_owned(), data.trim_start().to_owned()));
        }
    }

    fn open_tag<'a>(&mut self, rest: &'a str) -> Result<&'a str, String> {
        let end = rest
            .find(|c: char| c.is_ascii_whitespace() || c == '/' || c == '>')
            .ok_or(MALFORMED)?;
        let name = rest.get(..end).ok_or(MALFORMED)?;
        let tag = start_tag(rest.get(end..).ok_or(MALFORMED)?)?;
        if !is_qname(name) || prefix(name) == "xmlns" {
            return Err(MALFORMED.to_owned());
        }
        self.elements += 1;
        if self.elements > self.limits.elements || self.open.len() >= self.limits.depth {
            return Err("it is nested too deep or has too many elements".to_owned());
        }
        if self.root.is_some() && self.open.is_empty() {
            return Err("it has more than one document element".to_owned());
        }
        self.marks.push(self.scope.len());
        let element = self.element(name, tag.attrs)?;
        self.open.push(element);
        if tag.empty {
            self.close(name)?;
        }
        Ok(tag.rest)
    }

    /// Builds an element from its name and its attributes as written, its namespace declarations put in scope.
    fn element(&mut self, name: &str, raw: Vec<(&str, String)>) -> Result<Element, String> {
        let mut element = Element {
            name: name.to_owned(),
            ..Element::default()
        };
        let mut plain = Vec::new();
        for (qname, value) in raw {
            let declared = match (prefix(qname), local(qname)) {
                ("", "xmlns") => Some(""),
                ("xmlns", p) => Some(p),
                _ => None,
            };
            match declared {
                Some("xmlns") => return Err(MALFORMED.to_owned()),
                // `xml` is bound by definition; declaring it again changes nothing, and anything else is an error.
                Some("xml") if value == XML_NS => {}
                Some(p) if (p == "xml") || (!p.is_empty() && value.is_empty()) => {
                    return Err(MALFORMED.to_owned());
                }
                Some(p) => {
                    self.scope.push((p.to_owned(), value.clone()));
                    element.decls.push((p.to_owned(), value));
                }
                None => plain.push((qname, value)),
            }
        }
        element.uri = self.resolve(prefix(name))?.to_owned();
        for (qname, value) in plain {
            let uri = match prefix(qname) {
                "" => String::new(),
                p => self.resolve(p)?.to_owned(),
            };
            let twin = element
                .attrs
                .iter()
                .any(|a| a.name == qname || (a.uri == uri && local(&a.name) == local(qname)));
            if twin {
                return Err("an element repeats an attribute".to_owned());
            }
            element.attrs.push(Attr {
                name: qname.to_owned(),
                uri,
                value,
            });
        }
        Ok(element)
    }

    /// The namespace `prefix` is bound to; the default namespace is empty when none is declared.
    fn resolve(&self, prefix: &str) -> Result<&str, String> {
        if prefix == "xml" {
            return Ok(XML_NS);
        }
        match self.scope.iter().rev().find(|(p, _)| p == prefix) {
            Some((_, uri)) => Ok(uri),
            None if prefix.is_empty() => Ok(""),
            None => Err(format!("the prefix {prefix} is not declared")),
        }
    }

    fn close(&mut self, name: &str) -> Result<(), String> {
        let element = self.open.pop().ok_or(MALFORMED)?;
        if element.name != name {
            return Err("an end tag does not match its start tag".to_owned());
        }
        self.scope.truncate(self.marks.pop().unwrap_or(0));
        match self.open.last_mut() {
            Some(parent) => parent.children.push(Node::Element(element)),
            None => self.root = Some(element),
        }
        Ok(())
    }
}

/// Appends text to an element, joined to the text before it.
fn push_text(parent: &mut Element, text: &str) {
    if let Some(Node::Text(t)) = parent.children.last_mut() {
        t.push_str(text);
    } else {
        parent.children.push(Node::Text(text.to_owned()));
    }
}

/// A start tag, from just after its name.
struct StartTag<'a> {
    /// Each attribute's name as written, and its normalized value.
    attrs: Vec<(&'a str, String)>,
    /// Whether the tag closes itself (`/>`).
    empty: bool,
    /// What follows the tag.
    rest: &'a str,
}

fn start_tag(mut s: &str) -> Result<StartTag<'_>, String> {
    let mut out = Vec::new();
    loop {
        s = s.trim_start();
        if let Some(rest) = s.strip_prefix("/>") {
            return Ok(StartTag {
                attrs: out,
                empty: true,
                rest,
            });
        }
        if let Some(rest) = s.strip_prefix('>') {
            return Ok(StartTag {
                attrs: out,
                empty: false,
                rest,
            });
        }
        let eq = s.find('=').ok_or(MALFORMED)?;
        let name = s.get(..eq).ok_or(MALFORMED)?.trim_end();
        if !is_qname(name) {
            return Err(MALFORMED.to_owned());
        }
        s = s.get(eq + 1..).ok_or(MALFORMED)?.trim_start();
        let quote = s
            .chars()
            .next()
            .filter(|c| *c == '"' || *c == '\'')
            .ok_or(MALFORMED)?;
        let body = s.get(1..).ok_or(MALFORMED)?;
        let end = body.find(quote).ok_or(MALFORMED)?;
        let raw = body.get(..end).ok_or(MALFORMED)?;
        if raw.contains('<') {
            return Err(MALFORMED.to_owned());
        }
        // Attribute-value normalization: a literal tab or line end is a space; a character reference keeps its character.
        let spaced = raw.replace(['\t', '\n'], " ");
        out.push((name, unescape(&spaced).ok_or(MALFORMED)?));
        s = body.get(end + 1..).ok_or(MALFORMED)?;
        if !(s.starts_with(|c: char| c.is_ascii_whitespace())
            || s.starts_with('>')
            || s.starts_with("/>"))
        {
            return Err(MALFORMED.to_owned());
        }
    }
}

/// Whether `name` is a plausible qualified name: not empty, at most one colon, neither first nor last.
fn is_qname(name: &str) -> bool {
    let bad = |c: char| c.is_whitespace() || "<>&/=\"'".contains(c);
    !name.is_empty()
        && !name.contains(bad)
        && name.matches(':').count() <= 1
        && !name.starts_with(':')
        && !name.ends_with(':')
}

/// Text with XML's entities and character references resolved. An unknown entity, or a reference to a character XML does not
/// allow, is an error.
fn unescape(s: &str) -> Option<String> {
    if !s.contains('&') {
        return Some(s.to_owned());
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(rest.get(..amp)?);
        rest = rest.get(amp + 1..)?;
        let semi = rest.find(';')?;
        let c = match rest.get(..semi)? {
            "amp" => '&',
            "lt" => '<',
            "gt" => '>',
            "quot" => '"',
            "apos" => '\'',
            entity => {
                let code = if let Some(hex) = entity.strip_prefix("#x") {
                    u32::from_str_radix(hex, 16).ok()?
                } else {
                    entity.strip_prefix('#')?.parse().ok()?
                };
                char::from_u32(code).filter(|c| *c != '\0')?
            }
        };
        out.push(c);
        rest = rest.get(semi + 1..)?;
    }
    out.push_str(rest);
    Some(out)
}
