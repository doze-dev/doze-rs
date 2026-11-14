//! A small XML reader for request bodies: elements, text, entities and CDATA into a tree. It reads what S3 clients send and refuses
//! the rest: a document type declaration (the door to entity attacks) is an error, namespaces are dropped (`xsi:type` is `type`),
//! attributes are kept by their local name (namespace declarations are not), and depth and size are capped so a hostile body cannot overflow the stack or the heap.

/// The deepest nesting read: S3 bodies nest about eight levels.
const MAX_DEPTH: usize = 40;
/// The most elements read from one body.
const MAX_ELEMENTS: usize = 400_000;

/// One element: its name without a namespace prefix, its text, and its child elements in order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Elem {
    /// The local name.
    pub name: String,
    /// The text directly inside it (entities resolved), with whitespace between children dropped.
    pub text: String,
    /// The child elements.
    pub children: Vec<Elem>,
    /// The attributes, by local name (`xsi:type` is `type`), entities resolved; `xmlns` declarations are left out.
    pub attrs: Vec<(String, String)>,
}

/// Why a body is not XML this reader accepts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("the XML is not well formed or not accepted")]
pub struct XmlError;

struct Reader<'a> {
    s: &'a [u8],
    at: usize,
    elements: usize,
}

// fuzz:parser fuzz_xml
/// Reads the document element of `input`.
///
/// # Errors
/// [`XmlError`] for anything that is not one well-formed element (with an optional declaration and comments around it).
pub fn parse(input: &[u8]) -> Result<Elem, XmlError> {
    let mut r = Reader {
        s: input,
        at: 0,
        elements: 0,
    };
    r.skip_misc()?;
    let root = r.element(0)?;
    r.skip_misc()?;
    if r.at == r.s.len() {
        Ok(root)
    } else {
        Err(XmlError)
    }
}

impl Reader<'_> {
    fn peek(&self) -> Option<u8> {
        self.s.get(self.at).copied()
    }

    fn starts(&self, pat: &[u8]) -> bool {
        self.s.get(self.at..).is_some_and(|r| r.starts_with(pat))
    }

    fn skip_space(&mut self) {
        while self.peek().is_some_and(|b| b.is_ascii_whitespace()) {
            self.at += 1;
        }
    }

    /// Skips up to and including `pat`.
    fn skip_past(&mut self, pat: &[u8]) -> Result<(), XmlError> {
        let rest = self.s.get(self.at..).ok_or(XmlError)?;
        let found = rest
            .windows(pat.len())
            .position(|w| w == pat)
            .ok_or(XmlError)?;
        self.at += found + pat.len();
        Ok(())
    }

    /// Whitespace, the XML declaration, processing instructions and comments, between or around elements.
    fn skip_misc(&mut self) -> Result<(), XmlError> {
        loop {
            self.skip_space();
            if self.starts(b"<?") {
                self.skip_past(b"?>")?;
            } else if self.starts(b"<!--") {
                self.skip_past(b"-->")?;
            } else if self.starts(b"<!") {
                return Err(XmlError);
            } else {
                return Ok(());
            }
        }
    }

    fn name(&mut self) -> Result<&str, XmlError> {
        let start = self.at;
        while self
            .peek()
            .is_some_and(|b| !b.is_ascii_whitespace() && !matches!(b, b'>' | b'/' | b'=' | b'<'))
        {
            self.at += 1;
        }
        let raw = self.s.get(start..self.at).ok_or(XmlError)?;
        let name = std::str::from_utf8(raw).map_err(|_| XmlError)?;
        if name.is_empty() {
            return Err(XmlError);
        }
        Ok(name.rsplit(':').next().unwrap_or(name))
    }

    fn element(&mut self, depth: usize) -> Result<Elem, XmlError> {
        self.elements += 1;
        if depth > MAX_DEPTH || self.elements > MAX_ELEMENTS || self.peek() != Some(b'<') {
            return Err(XmlError);
        }
        self.at += 1;
        let mut elem = Elem {
            name: self.name()?.to_owned(),
            ..Elem::default()
        };
        if self.attributes(&mut elem.attrs)? {
            return Ok(elem);
        }
        self.content(&mut elem, depth)?;
        Ok(elem)
    }

    /// Reads the attributes; true when the tag closes itself (`<a/>`).
    fn attributes(&mut self, attrs: &mut Vec<(String, String)>) -> Result<bool, XmlError> {
        loop {
            self.skip_space();
            match self.peek().ok_or(XmlError)? {
                b'>' => {
                    self.at += 1;
                    return Ok(false);
                }
                b'/' => {
                    self.at += 1;
                    if self.peek() != Some(b'>') {
                        return Err(XmlError);
                    }
                    self.at += 1;
                    return Ok(true);
                }
                _ => self.attribute(attrs)?,
            }
        }
    }

    fn attribute(&mut self, attrs: &mut Vec<(String, String)>) -> Result<(), XmlError> {
        let start = self.at;
        let local = self.name()?.to_owned();
        let declares = self
            .s
            .get(start..self.at)
            .is_some_and(|raw| raw == b"xmlns" || raw.starts_with(b"xmlns:"));
        self.skip_space();
        if self.peek() != Some(b'=') {
            return Err(XmlError);
        }
        self.at += 1;
        self.skip_space();
        let quote = self
            .peek()
            .filter(|q| matches!(q, b'"' | b'\''))
            .ok_or(XmlError)?;
        self.at += 1;
        let from = self.at;
        self.skip_past(&[quote])?;
        let raw = self.s.get(from..self.at - 1).ok_or(XmlError)?;
        let mut value = String::new();
        unescape(std::str::from_utf8(raw).map_err(|_| XmlError)?, &mut value)?;
        if !declares && attrs.len() < 16 {
            attrs.push((local, value));
        }
        Ok(())
    }

    fn content(&mut self, elem: &mut Elem, depth: usize) -> Result<(), XmlError> {
        loop {
            if self.starts(b"</") {
                self.at += 2;
                let close = self.name()?.to_owned();
                self.skip_space();
                if close != elem.name || self.peek() != Some(b'>') {
                    return Err(XmlError);
                }
                self.at += 1;
                if !elem.children.is_empty() && elem.text.trim().is_empty() {
                    elem.text.clear();
                }
                return Ok(());
            } else if self.starts(b"<![CDATA[") {
                self.at += 9;
                let start = self.at;
                self.skip_past(b"]]>")?;
                let raw = self.s.get(start..self.at - 3).ok_or(XmlError)?;
                elem.text
                    .push_str(std::str::from_utf8(raw).map_err(|_| XmlError)?);
            } else if self.starts(b"<!--") {
                self.skip_past(b"-->")?;
            } else if self.starts(b"<?") {
                self.skip_past(b"?>")?;
            } else if self.starts(b"<!") {
                return Err(XmlError);
            } else if self.peek() == Some(b'<') {
                let child = self.element(depth + 1)?;
                elem.children.push(child);
            } else if self.peek().is_some() {
                self.text(&mut elem.text)?;
            } else {
                return Err(XmlError);
            }
        }
    }

    /// Text up to the next `<`, with entities resolved.
    fn text(&mut self, out: &mut String) -> Result<(), XmlError> {
        let start = self.at;
        while self.peek().is_some_and(|b| b != b'<') {
            self.at += 1;
        }
        let raw = self.s.get(start..self.at).ok_or(XmlError)?;
        let text = std::str::from_utf8(raw).map_err(|_| XmlError)?;
        unescape(text, out)
    }
}

/// `&lt;` and the other entities in `text`, appended to `out`.
fn unescape(text: &str, out: &mut String) -> Result<(), XmlError> {
    let mut rest = text;
    while let Some(i) = rest.find('&') {
        out.push_str(rest.get(..i).ok_or(XmlError)?);
        let tail = rest.get(i + 1..).ok_or(XmlError)?;
        let end = tail.find(';').ok_or(XmlError)?;
        let entity = tail.get(..end).ok_or(XmlError)?;
        out.push(match entity {
            "lt" => '<',
            "gt" => '>',
            "amp" => '&',
            "quot" => '"',
            "apos" => '\'',
            other => char_ref(other)?,
        });
        rest = tail.get(end + 1..).ok_or(XmlError)?;
    }
    out.push_str(rest);
    Ok(())
}

fn char_ref(entity: &str) -> Result<char, XmlError> {
    let digits = entity.strip_prefix('#').ok_or(XmlError)?;
    let code = match digits.strip_prefix(['x', 'X']) {
        Some(hex) => u32::from_str_radix(hex, 16),
        None => digits.parse(),
    };
    code.ok().and_then(char::from_u32).ok_or(XmlError)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_document_becomes_a_tree() {
        let e = parse(
            b"<?xml version=\"1.0\"?><A xmlns=\"u\" x:y='1'><B>1 &lt; 2</B><C/><B><![CDATA[<z>]]></B></A>",
        )
        .unwrap();
        assert_eq!(e.name, "A");
        let texts: Vec<&str> = e.children.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(texts, ["1 < 2", "", "<z>"]);
    }

    #[test]
    fn attributes_are_kept_by_local_name_without_namespace_declarations() {
        let e = parse(
            b"<Grantee xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:type=\"Group\" a='&amp;'><URI>u</URI></Grantee>",
        )
        .unwrap();
        assert_eq!(
            e.attrs,
            [
                ("type".to_owned(), "Group".to_owned()),
                ("a".to_owned(), "&".to_owned())
            ]
        );
    }

    #[test]
    fn a_prefix_is_dropped_and_character_references_work() {
        let e = parse(b"<x:A>&#65;&#x42;</x:A>").unwrap();
        assert_eq!((e.name.as_str(), e.text.as_str()), ("A", "AB"));
    }

    #[test]
    fn what_is_not_one_element_is_refused() {
        for bad in [
            &b""[..],
            b"<A>",
            b"<A></B>",
            b"<A/><B/>",
            b"<!DOCTYPE a [<!ENTITY x \"y\">]><A>&x;</A>",
            b"<A>&nope;</A>",
            b"<A b=c/>",
            b"text",
        ] {
            assert!(parse(bad).is_err(), "{}", String::from_utf8_lossy(bad));
        }
    }

    #[test]
    fn nesting_is_capped() {
        let deep = "<a>".repeat(200) + &"</a>".repeat(200);
        assert!(parse(deep.as_bytes()).is_err());
    }
}
