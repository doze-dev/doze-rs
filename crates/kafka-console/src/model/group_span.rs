//! A word of a sentence of the Groups pages.

/// A word of a sentence of the Groups pages: plain, monospace, bold, or a link to a topic.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "kind", content = "text", rename_all = "snake_case")]
pub enum GroupSpan {
    Text(String),
    Mono(String),
    Strong(String),
    /// A topic's name, linked to its page.
    Topic(String),
}

impl GroupSpan {
    pub fn text(s: impl Into<String>) -> Self {
        Self::Text(s.into())
    }

    pub fn mono(s: impl Into<String>) -> Self {
        Self::Mono(s.into())
    }

    pub fn strong(s: impl Into<String>) -> Self {
        Self::Strong(s.into())
    }

    pub fn topic(s: impl Into<String>) -> Self {
        Self::Topic(s.into())
    }
}
