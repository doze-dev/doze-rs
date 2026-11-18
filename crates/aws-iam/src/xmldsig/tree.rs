//! The document a signature is checked in, read into a tree that keeps what canonicalization needs: each element's name as
//! written and the namespace it resolves to, its namespace declarations, its attributes in order, and its text and processing
//! instructions. Comments are dropped (exclusive canonicalization without comments does not render them). The reader that
//! builds it is in `reader`.

/// How much of a document is read.
#[derive(Debug, Clone, Copy)]
pub(super) struct Limits {
    pub(super) bytes: usize,
    pub(super) depth: usize,
    pub(super) elements: usize,
}

/// An element, with what it holds.
#[derive(Debug, Default)]
pub(super) struct Element {
    /// The qualified name as written (`saml:Assertion`).
    pub(super) name: String,
    /// The namespace the name is in; empty for none.
    pub(super) uri: String,
    /// The namespace declarations made on this element, in document order: the prefix (empty for the default) and the URI
    /// (empty for `xmlns=""`).
    pub(super) decls: Vec<(String, String)>,
    pub(super) attrs: Vec<Attr>,
    pub(super) children: Vec<Node>,
}

/// An attribute other than a namespace declaration.
#[derive(Debug)]
pub(super) struct Attr {
    /// The qualified name as written.
    pub(super) name: String,
    /// The namespace of a prefixed name; empty for an unprefixed one.
    pub(super) uri: String,
    pub(super) value: String,
}

#[derive(Debug)]
pub(super) enum Node {
    Element(Element),
    Text(String),
    /// A processing instruction: its target and its data.
    Pi(String, String),
}

impl Element {
    pub(super) fn local(&self) -> &str {
        local(&self.name)
    }

    /// Whether this is the element `local` in the namespace `uri`.
    pub(super) fn is(&self, uri: &str, local: &str) -> bool {
        self.uri == uri && self.local() == local
    }

    /// The value of the unprefixed attribute `name`.
    pub(super) fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|a| a.name == name)
            .map(|a| a.value.as_str())
    }

    /// The child elements, in order.
    pub(super) fn elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|n| match n {
            Node::Element(e) => Some(e),
            _ => None,
        })
    }

    /// The child elements named `local` in `uri`.
    pub(super) fn named<'a>(
        &'a self,
        uri: &'a str,
        local: &'a str,
    ) -> impl Iterator<Item = &'a Element> {
        self.elements().filter(move |e| e.is(uri, local))
    }

    /// The text directly inside this element.
    pub(super) fn text(&self) -> String {
        self.children
            .iter()
            .filter_map(|n| match n {
                Node::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect()
    }

    /// This element and every element inside it, depth first.
    pub(super) fn walk(&self, each: &mut impl FnMut(&Element)) {
        each(self);
        for e in self.elements() {
            e.walk(each);
        }
    }
}

/// The name without its prefix.
pub(super) fn local(name: &str) -> &str {
    name.split_once(':').map_or(name, |(_, l)| l)
}

/// The prefix of a name; empty for none.
pub(super) fn prefix(name: &str) -> &str {
    name.split_once(':').map_or("", |(p, _)| p)
}
