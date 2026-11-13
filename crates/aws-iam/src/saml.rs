//! What `AssumeRoleWithSAML` reads from an assertion: the subject (`NameID` and its format), the issuer, the recipient and audience,
//! and the AWS attributes (`RoleSessionName`, `SourceIdentity`). **The signature is not checked**: the assertion is read, not
//! trusted, as the Go edition did; a local account has no identity provider whose key it could check against.
//!
//! The reader is small and bounded, for this one job: it walks tags and text, keeps attributes (which `aws-wire`'s XML reader
//! drops), refuses a document type declaration, and stops at [`MAX_BYTES`], [`MAX_DEPTH`] and [`MAX_ELEMENTS`].

use std::collections::BTreeMap;

/// The largest assertion read, decoded (the model allows 100 000 characters of base64).
const MAX_BYTES: usize = 100_000;
const MAX_DEPTH: usize = 32;
const MAX_ELEMENTS: usize = 10_000;

const ROLE_SESSION_NAME: &str = "https://aws.amazon.com/SAML/Attributes/RoleSessionName";
const SOURCE_IDENTITY: &str = "https://aws.amazon.com/SAML/Attributes/SourceIdentity";

/// What an assertion says.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Assertion {
    pub(crate) issuer: String,
    pub(crate) name_id: String,
    /// The `NameID`'s `Format` attribute.
    pub(crate) format: String,
    /// The `Recipient` of `SubjectConfirmationData`.
    pub(crate) recipient: String,
    /// The first `Audience` of the conditions.
    pub(crate) audience: String,
    pub(crate) role_session_name: Option<String>,
    pub(crate) source_identity: Option<String>,
}

impl Assertion {
    /// `SubjectType` as AWS words it: the format without `urn:oasis:names:tc:SAML:2.0:nameid-format:` (`persistent`,
    /// `transient`), any other format as given (memory: from the STS reference, not recorded).
    pub(crate) fn subject_type(&self) -> String {
        self.format
            .strip_prefix("urn:oasis:names:tc:SAML:2.0:nameid-format:")
            .unwrap_or(&self.format)
            .to_owned()
    }
}

/// Reads a base64 assertion (line breaks allowed). `None` for anything that is not base64 of a well-formed XML document within the
/// bounds.
pub(crate) fn read(base64: &str) -> Option<Assertion> {
    let compact: String = base64
        .chars()
        .filter(|c| !c.is_ascii_whitespace())
        .collect();
    let bytes = aws_core::base64::decode(&compact)?;
    if bytes.len() > MAX_BYTES {
        return None;
    }
    let text = String::from_utf8(bytes).ok()?;
    let mut walk = Walk::default();
    let mut rest = text.as_str();
    while let Some(lt) = rest.find('<') {
        walk.text(&unescape(rest.get(..lt)?)?);
        rest = walk.markup(rest.get(lt..)?)?;
    }
    let out = walk.out;
    if !walk.stack.is_empty() || (out.name_id.is_empty() && out.issuer.is_empty()) {
        return None;
    }
    Some(out)
}

/// The walk through a document: where it is, and what it has kept.
#[derive(Default)]
struct Walk {
    out: Assertion,
    /// The open elements, outermost first.
    stack: Vec<String>,
    /// The attribute an `AttributeValue` belongs to, while inside one `Attribute`.
    attribute: Option<String>,
    elements: usize,
    /// The text of the innermost open element so far.
    text_of: Option<String>,
}

impl Walk {
    fn text(&mut self, s: &str) {
        if let Some(t) = self.text_of.as_mut() {
            t.push_str(s);
        }
    }

    /// Reads the markup `rest` starts with (`<...`), and gives back what follows it.
    fn markup<'a>(&mut self, rest: &'a str) -> Option<&'a str> {
        if let Some(after) = rest.strip_prefix("<!--") {
            return after.get(after.find("-->")? + 3..);
        }
        if let Some(after) = rest.strip_prefix("<![CDATA[") {
            let end = after.find("]]>")?;
            self.text(after.get(..end)?);
            return after.get(end + 3..);
        }
        if rest.starts_with("<!") {
            // A document type declaration, the door to entity expansion: refused.
            return None;
        }
        if let Some(after) = rest.strip_prefix("<?") {
            return after.get(after.find("?>")? + 2..);
        }
        let gt = rest.find('>')?;
        let tag = rest.get(1..gt)?;
        match tag.strip_prefix('/') {
            Some(name) => self.close(local(name.trim()))?,
            None => self.open(tag)?,
        }
        rest.get(gt + 1..)
    }

    fn close(&mut self, name: &str) -> Option<()> {
        if self.stack.pop().as_deref() != Some(name) {
            return None;
        }
        if let Some(t) = self.text_of.take() {
            self.out
                .take_text(&self.stack, name, self.attribute.as_deref(), t.trim());
        }
        if name == "Attribute" {
            self.attribute = None;
        }
        Some(())
    }

    fn open(&mut self, tag: &str) -> Option<()> {
        let empty = tag.ends_with('/');
        let tag = tag.trim_end_matches('/');
        let (name, attrs) = tag
            .split_once(|c: char| c.is_ascii_whitespace())
            .unwrap_or((tag, ""));
        let name = local(name);
        let attrs = attributes(attrs)?;
        self.elements += 1;
        if self.elements > MAX_ELEMENTS || self.stack.len() >= MAX_DEPTH || name.is_empty() {
            return None;
        }
        let out = &mut self.out;
        match name {
            "NameID" if self.stack.last().map(String::as_str) == Some("Subject") => {
                out.format = attrs.get("Format").cloned().unwrap_or_default();
            }
            "SubjectConfirmationData" if out.recipient.is_empty() => {
                out.recipient = attrs.get("Recipient").cloned().unwrap_or_default();
            }
            "Attribute" => self.attribute = attrs.get("Name").cloned(),
            _ => {}
        }
        if !empty {
            self.stack.push(name.to_owned());
            self.text_of = Some(String::new());
        }
        Some(())
    }
}

impl Assertion {
    /// Keeps the text of an element just closed, by where it was.
    fn take_text(&mut self, parents: &[String], name: &str, attribute: Option<&str>, text: &str) {
        let parent = parents.last().map(String::as_str);
        match name {
            // The assertion's own issuer wins over the response's.
            "Issuer" if parent == Some("Assertion") || self.issuer.is_empty() => {
                self.issuer = text.to_owned();
            }
            "NameID" if parent == Some("Subject") => self.name_id = text.to_owned(),
            "Audience" if self.audience.is_empty() => self.audience = text.to_owned(),
            "AttributeValue" => {
                let slot = match attribute {
                    Some(ROLE_SESSION_NAME) => &mut self.role_session_name,
                    Some(SOURCE_IDENTITY) => &mut self.source_identity,
                    _ => return,
                };
                if slot.is_none() {
                    *slot = Some(text.to_owned());
                }
            }
            _ => {}
        }
    }
}

/// The name without its namespace prefix.
fn local(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

/// `a="1" b='2'`, names without their prefixes, values unescaped.
fn attributes(mut s: &str) -> Option<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    loop {
        s = s.trim_start();
        if s.is_empty() {
            return Some(out);
        }
        let eq = s.find('=')?;
        let name = local(s.get(..eq)?.trim()).to_owned();
        s = s.get(eq + 1..)?.trim_start();
        let quote = s.chars().next().filter(|c| *c == '"' || *c == '\'')?;
        let body = s.get(1..)?;
        let end = body.find(quote)?;
        out.insert(name, unescape(body.get(..end)?)?);
        s = body.get(end + 1..)?;
    }
}

/// Text with XML's entities resolved. An unknown entity is an error.
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
        let entity = rest.get(..semi)?;
        let c = match entity {
            "amp" => '&',
            "lt" => '<',
            "gt" => '>',
            "quot" => '"',
            "apos" => '\'',
            _ => {
                let code = if let Some(hex) = entity.strip_prefix("#x") {
                    u32::from_str_radix(hex, 16).ok()?
                } else {
                    entity.strip_prefix('#')?.parse().ok()?
                };
                char::from_u32(code)?
            }
        };
        out.push(c);
        rest = rest.get(semi + 1..)?;
    }
    out.push_str(rest);
    Some(out)
}

#[cfg(test)]
pub(crate) mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    /// An assertion as an identity provider sends one (signature elided: it is not read).
    pub(crate) const ASSERTION: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<samlp:Response xmlns:samlp="urn:oasis:names:tc:SAML:2.0:protocol" ID="_r1" Version="2.0">
  <saml:Issuer xmlns:saml="urn:oasis:names:tc:SAML:2.0:assertion">https://idp.example.com/response</saml:Issuer>
  <saml:Assertion xmlns:saml="urn:oasis:names:tc:SAML:2.0:assertion" ID="_a1" Version="2.0">
    <saml:Issuer>https://idp.example.com/saml</saml:Issuer>
    <!-- a comment -->
    <saml:Subject>
      <saml:NameID Format="urn:oasis:names:tc:SAML:2.0:nameid-format:persistent">alice&amp;co</saml:NameID>
      <saml:SubjectConfirmation Method="urn:oasis:names:tc:SAML:2.0:cm:bearer">
        <saml:SubjectConfirmationData Recipient="https://signin.aws.amazon.com/saml" NotOnOrAfter="2030-01-01T00:00:00Z"/>
      </saml:SubjectConfirmation>
    </saml:Subject>
    <saml:Conditions>
      <saml:AudienceRestriction><saml:Audience>urn:amazon:webservices</saml:Audience></saml:AudienceRestriction>
    </saml:Conditions>
    <saml:AttributeStatement>
      <saml:Attribute Name="https://aws.amazon.com/SAML/Attributes/Role">
        <saml:AttributeValue>arn:aws:iam::000000000000:role/r,arn:aws:iam::000000000000:saml-provider/idp</saml:AttributeValue>
      </saml:Attribute>
      <saml:Attribute Name="https://aws.amazon.com/SAML/Attributes/RoleSessionName">
        <saml:AttributeValue><![CDATA[alice@example.com]]></saml:AttributeValue>
      </saml:Attribute>
    </saml:AttributeStatement>
  </saml:Assertion>
</samlp:Response>"#;

    #[test]
    fn an_assertion_gives_its_subject_issuer_recipient_and_session_name() {
        let a = read(&aws_core::base64::encode(ASSERTION.as_bytes())).unwrap();
        assert_eq!(a.issuer, "https://idp.example.com/saml");
        assert_eq!(a.name_id, "alice&co");
        assert_eq!(a.subject_type(), "persistent");
        assert_eq!(a.recipient, "https://signin.aws.amazon.com/saml");
        assert_eq!(a.audience, "urn:amazon:webservices");
        assert_eq!(a.role_session_name.as_deref(), Some("alice@example.com"));
        assert_eq!(a.source_identity, None);
    }

    #[test]
    fn line_breaks_in_the_base64_are_allowed() {
        let coded = aws_core::base64::encode(ASSERTION.as_bytes());
        let wrapped: String = coded
            .as_bytes()
            .chunks(76)
            .map(|c| format!("{}\r\n", std::str::from_utf8(c).unwrap()))
            .collect();
        assert!(read(&wrapped).is_some());
    }

    #[test]
    fn anything_else_is_refused() {
        let b64 = |s: &str| aws_core::base64::encode(s.as_bytes());
        for bad in [
            "not base64 at all!".to_owned(),
            b64("not xml"),
            b64("<a><b></a>"),
            b64(r#"<!DOCTYPE a [<!ENTITY x "y">]><a><Issuer>&x;</Issuer></a>"#),
            b64("<Response><Issuer>i</Issuer>"),
            b64(&"<a>".repeat(MAX_DEPTH + 1)),
        ] {
            assert!(read(&bad).is_none(), "{bad}");
        }
    }
}
