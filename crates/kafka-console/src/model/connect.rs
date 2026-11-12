//! The view-models of Connect and of the advertised-listener check.

/// A run of text in a sentence: plain, or in the monospace face (a name, an address).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Span {
    Text(String),
    Mono(String),
    /// Bold, as the sentence under a page title writes a figure.
    Strong(String),
}

impl Span {
    pub fn text(s: impl Into<String>) -> Self {
        Self::Text(s.into())
    }

    pub fn mono(s: impl Into<String>) -> Self {
        Self::Mono(s.into())
    }

    pub fn strong(s: impl Into<String>) -> Self {
        Self::Strong(s.into())
    }
}

/// One line of the checklist: a verdict, what was checked, what was done to check it, and the figure beside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub ok: bool,
    pub title: String,
    pub detail: Vec<Span>,
    /// `2 ms`, `Metadata`, `failed`.
    pub value: String,
}

/// What the three rules add up to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Reachable,
    NotReachable,
    /// The check did not finish in time (or has not run): nothing is claimed.
    NotChecked,
}

/// The advertised-listener check, as every page that shows it needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeView {
    pub verdict: Verdict,
    pub rules: Vec<Rule>,
    /// Where the broker listens, `127.0.0.1:9092`.
    pub listen: String,
    /// What clients are told to reconnect to.
    pub advertised: String,
    /// Whether the two are the same address.
    pub same: bool,
    /// The version of Metadata the check sent.
    pub metadata_version: i16,
}

/// What a client needs to log in, as far as the page says it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Access {
    /// The word of the security chip: `Open`, `SASL SCRAM · ACLs + TLS`.
    pub label: String,
    /// Whether a client must log in.
    pub sasl: bool,
    /// The second line of the Access row on Home: `No sign-in, no ACLs, plain TCP`.
    pub detail: String,
}

/// One client's snippet: its name, the language tag and the code with the address in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientSnippet {
    pub name: &'static str,
    pub tag: &'static str,
    /// Lines of code; each token is plain, a keyword, a string or a comment.
    pub code: Vec<Vec<(Tok, String)>>,
}

/// A colour class of a snippet token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tok {
    Plain,
    Keyword,
    Str,
    Comment,
}

/// The Connect page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectView {
    pub listen: String,
    pub loopback: bool,
    pub probe: ProbeView,
    pub access: Access,
    pub snippets: Vec<ClientSnippet>,
}
