//! Exclusive XML Canonicalization 1.0, without comments (<https://www.w3.org/TR/xml-exc-c14n/>): the bytes a digest and a
//! signature are computed over. An element subtree is rendered with only the namespace declarations it visibly uses (its own
//! prefix, its attributes' prefixes, and those the `InclusiveNamespaces PrefixList` names), each rendered once where an output
//! ancestor has not already rendered it with the same value. Declarations come sorted by prefix (the default first), attributes
//! by namespace URI and then local name (unqualified ones first), empty elements as a start and an end tag.

use super::tree::{Element, Node, local, prefix};

/// Exclusive canonicalization without comments, the one method signatures here may name.
pub(super) const EXC_C14N: &str = "http://www.w3.org/2001/10/xml-exc-c14n#";
/// The variant that keeps comments: the tree does not keep them, so it is refused by name.
pub(super) const EXC_C14N_COMMENTS: &str = "http://www.w3.org/2001/10/xml-exc-c14n#WithComments";

/// The canonical form of the last element of `path` (the elements from the document element down to it, whose declarations
/// are in scope), leaving out the subtree `omit` (the enveloped signature), with the prefixes `inclusive` names (`#default`
/// for the default namespace) handled as inclusive canonicalization would.
pub(super) fn canonicalize(
    path: &[&Element],
    omit: Option<&Element>,
    inclusive: &[String],
) -> String {
    let Some((apex, ancestors)) = path.split_last() else {
        return String::new();
    };
    let mut canon = Canon {
        out: String::new(),
        scope: ancestors
            .iter()
            .flat_map(|e| e.decls.iter().map(|(p, u)| (p.as_str(), u.as_str())))
            .collect(),
        rendered: Vec::new(),
        omit,
        inclusive: inclusive
            .iter()
            .map(|p| if p == "#default" { "" } else { p.as_str() })
            .collect(),
    };
    canon.element(apex);
    canon.out
}

struct Canon<'a> {
    out: String,
    /// The namespace declarations in scope: prefix and URI, innermost last.
    scope: Vec<(&'a str, &'a str)>,
    /// The declarations output ancestors have rendered, innermost last.
    rendered: Vec<(&'a str, &'a str)>,
    omit: Option<&'a Element>,
    inclusive: Vec<&'a str>,
}

impl<'a> Canon<'a> {
    fn element(&mut self, e: &'a Element) {
        let (scoped, rendered) = (self.scope.len(), self.rendered.len());
        self.scope
            .extend(e.decls.iter().map(|(p, u)| (p.as_str(), u.as_str())));
        let decls = self.declarations(e);
        self.out.push('<');
        self.out.push_str(&e.name);
        for (p, uri) in &decls {
            self.out.push_str(" xmlns");
            if !p.is_empty() {
                self.out.push(':');
                self.out.push_str(p);
            }
            self.out.push_str("=\"");
            escape_attr(&mut self.out, uri);
            self.out.push('"');
        }
        self.rendered.extend(decls);
        let mut attrs: Vec<_> = e.attrs.iter().collect();
        attrs.sort_by(|a, b| (&a.uri, local(&a.name)).cmp(&(&b.uri, local(&b.name))));
        for a in attrs {
            self.out.push(' ');
            self.out.push_str(&a.name);
            self.out.push_str("=\"");
            escape_attr(&mut self.out, &a.value);
            self.out.push('"');
        }
        self.out.push('>');
        for child in &e.children {
            match child {
                Node::Element(c) if self.omit.is_some_and(|o| std::ptr::eq(o, c)) => {}
                Node::Element(c) => self.element(c),
                Node::Text(t) => escape_text(&mut self.out, t),
                Node::Pi(target, data) => {
                    self.out.push_str("<?");
                    self.out.push_str(target);
                    if !data.is_empty() {
                        self.out.push(' ');
                        self.out.push_str(data);
                    }
                    self.out.push_str("?>");
                }
            }
        }
        self.out.push_str("</");
        self.out.push_str(&e.name);
        self.out.push('>');
        self.scope.truncate(scoped);
        self.rendered.truncate(rendered);
    }

    /// The declarations `e` renders, sorted by prefix: each prefix it visibly uses (or the PrefixList names and is in scope)
    /// whose URI differs from what the nearest output ancestor rendered for it. The `xml` prefix is never declared.
    fn declarations(&self, e: &'a Element) -> Vec<(&'a str, &'a str)> {
        let mut used: Vec<&'a str> = vec![prefix(&e.name)];
        used.extend(
            e.attrs
                .iter()
                .map(|a| prefix(&a.name))
                .filter(|p| !p.is_empty()),
        );
        used.extend(
            self.inclusive
                .iter()
                .copied()
                .filter(|p| p.is_empty() || self.in_scope(p).is_some()),
        );
        used.sort_unstable();
        used.dedup();
        used.into_iter()
            .filter(|p| *p != "xml")
            .filter_map(|p| {
                let uri = self.in_scope(p).unwrap_or("");
                let before = self
                    .rendered
                    .iter()
                    .rev()
                    .find(|(q, _)| *q == p)
                    .map(|(_, u)| *u);
                // An absent default namespace and `xmlns=""` are the same thing.
                let same = if p.is_empty() {
                    before.unwrap_or("") == uri
                } else {
                    before == Some(uri)
                };
                (!same).then_some((p, uri))
            })
            .collect()
    }

    fn in_scope(&self, p: &str) -> Option<&'a str> {
        self.scope
            .iter()
            .rev()
            .find(|(q, _)| *q == p)
            .map(|(_, u)| *u)
    }
}

fn escape_text(out: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\r' => out.push_str("&#xD;"),
            c => out.push(c),
        }
    }
}

fn escape_attr(out: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '"' => out.push_str("&quot;"),
            '\t' => out.push_str("&#x9;"),
            '\n' => out.push_str("&#xA;"),
            '\r' => out.push_str("&#xD;"),
            c => out.push(c),
        }
    }
}

#[cfg(test)]
#[path = "c14n_tests.rs"]
mod tests;
