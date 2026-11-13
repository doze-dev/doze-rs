//! A module's console as every surface reads it. A module publishes a tree of rows, columns and blocks; which
//! layout a tab has and which blocks fill the room are decided here, so the CLI and the app draw the same console
//! from the same spec. Nothing here knows which module it is drawing.

use std::borrow::Cow;
use std::collections::HashMap;

/// A module's description of its console (`ConsoleSpec`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConsoleSpec {
    /// `panels`: the data a console draws.
    pub panels: Vec<Panel>,
    /// `tabs`: how the panels are laid out. A spec with panels and no tabs gets the default layout.
    pub tabs: Vec<ConsoleTab>,
}

impl ConsoleSpec {
    /// The panel named `id`. (Go: `PanelOf`.)
    #[must_use]
    pub fn panel(&self, id: &str) -> Option<&Panel> {
        self.panels.iter().find(|p| p.id == id)
    }
}

/// One tab of a console: a title and what is in it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConsoleTab {
    /// `id`.
    pub id: String,
    /// `title`.
    pub title: String,
    /// `body`.
    pub body: Node,
}

/// What a node of a layout is (`kind`): a container (`row`, `col`) or a block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeKind {
    /// Children side by side.
    Row,
    /// Children one under another.
    Col,
    /// A panel's rows as a table.
    Table,
    /// A panel's counters.
    Stat,
    /// The columns of one row, as key and value.
    Detail,
    /// A panel's rows as they arrive.
    Stream,
    /// The module's actions.
    Actions,
    /// A line of text.
    Note,
    /// A chart.
    Chart,
    /// A kind this crate does not know.
    Other(String),
}

impl Default for NodeKind {
    /// No kind, as a node the module sent without one.
    fn default() -> Self {
        Self::Other(String::new())
    }
}

impl NodeKind {
    /// Reads the module's word.
    #[must_use]
    pub fn parse(word: &str) -> Self {
        match word {
            "row" => Self::Row,
            "col" => Self::Col,
            "table" => Self::Table,
            "stat" => Self::Stat,
            "detail" => Self::Detail,
            "stream" => Self::Stream,
            "actions" => Self::Actions,
            "note" => Self::Note,
            "chart" => Self::Chart,
            other => Self::Other(other.to_owned()),
        }
    }

    /// The module's word.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Row => "row",
            Self::Col => "col",
            Self::Table => "table",
            Self::Stat => "stat",
            Self::Detail => "detail",
            Self::Stream => "stream",
            Self::Actions => "actions",
            Self::Note => "note",
            Self::Chart => "chart",
            Self::Other(word) => word,
        }
    }
}

/// A container or a block of a layout (`ConsoleNode`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Node {
    /// `kind`.
    pub kind: NodeKind,
    /// `id`.
    pub id: String,
    /// `title`.
    pub title: String,
    /// `weight`: its share of its parent's room; zero means one.
    pub weight: u32,
    /// `panel`: the panel a block draws.
    pub panel: String,
    /// `from`: the table block whose selected row it follows.
    pub from: String,
    /// `children`.
    pub children: Vec<Node>,
}

impl Node {
    /// Its name: its own id, else its kind and panel. (Go: `BlockID`.)
    #[must_use]
    pub fn block_id(&self) -> Cow<'_, str> {
        if self.id.is_empty() {
            Cow::Owned(format!("{}:{}", self.kind.as_str(), self.panel))
        } else {
            Cow::Borrowed(&self.id)
        }
    }

    /// It and every node under it, in order. (Go: `Walk`.)
    pub fn walk(&self) -> impl Iterator<Item = &Node> {
        let mut stack = vec![self];
        std::iter::from_fn(move || {
            let n = stack.pop()?;
            stack.extend(n.children.iter().rev());
            Some(n)
        })
    }

    /// Its share of its parent's room: one when it did not say. (Go: `Weight`.)
    #[must_use]
    pub fn share(&self) -> u32 {
        self.weight.max(1)
    }

    /// It takes the room that is left; the rest are as tall as their content. (Go: `Flexible`.)
    #[must_use]
    pub fn flexible(&self) -> bool {
        matches!(
            self.kind,
            NodeKind::Table | NodeKind::Stream | NodeKind::Row | NodeKind::Col
        )
    }

    /// It is or holds a stream. (Go: `HasStream`.)
    #[must_use]
    pub fn has_stream(&self) -> bool {
        self.walk().any(|n| n.kind == NodeKind::Stream)
    }

    pub(crate) fn block(kind: NodeKind, p: &Panel) -> Self {
        Self {
            kind,
            panel: p.id.clone(),
            title: p.title.clone(),
            weight: 1,
            ..Self::default()
        }
    }
}

/// What a panel holds (`kind`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PanelKind {
    /// `table`: rows, read whole each time.
    #[default]
    Table,
    /// `counters`: one row of figures.
    Counters,
    /// `stream`: rows one at a time.
    Stream,
    /// A kind this crate does not know.
    Other,
}

impl PanelKind {
    /// Reads the module's word.
    #[must_use]
    pub fn parse(word: &str) -> Self {
        match word {
            "table" => Self::Table,
            "counters" => Self::Counters,
            "stream" => Self::Stream,
            _ => Self::Other,
        }
    }
}

/// The data of a console (`ConsolePanel`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Panel {
    /// `id`.
    pub id: String,
    /// `title`.
    pub title: String,
    /// `kind`.
    pub kind: PanelKind,
    /// `columns`.
    pub columns: Vec<Column>,
    /// `select`: for a stream, the table whose selected row it follows.
    pub select: String,
}

impl Panel {
    /// The key of its first column: what names a row. (Go: `FirstCol`.)
    #[must_use]
    pub fn first_col(&self) -> &str {
        self.columns.first().map_or("", |c| c.key.as_str())
    }
}

/// What a column holds (`role`), which says how its cells are written.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Role {
    /// `count`: a figure with commas.
    Count,
    /// `bytes`: a size.
    Bytes,
    /// `rate`: worked out from successive totals.
    Rate,
    /// `lag`: green at zero, red above.
    Lag,
    /// `state`: a word with its glyph.
    State,
    /// Text as it is (`time`, `id`, `url`, `text`, none, or one this crate does not know).
    #[default]
    Text,
}

impl Role {
    /// Reads the module's word.
    #[must_use]
    pub fn parse(word: &str) -> Self {
        match word {
            "count" => Self::Count,
            "bytes" => Self::Bytes,
            "rate" => Self::Rate,
            "lag" => Self::Lag,
            "state" => Self::State,
            _ => Self::Text,
        }
    }
}

/// One column of a table, one field of a stream row (`ConsoleColumn`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Column {
    /// `key`.
    pub key: String,
    /// `title`.
    pub title: String,
    /// `role`.
    pub role: Role,
}

/// One row: its cells by column key, and a tone (`ConsoleRow`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConsoleRow {
    /// `cells`.
    pub cells: HashMap<String, String>,
    /// `tone`: `dim` for a row the module marks as not the point.
    pub tone: String,
}

impl ConsoleRow {
    /// The cell under `key`, empty when there is none.
    #[must_use]
    pub fn cell(&self, key: &str) -> &str {
        self.cells.get(key).map_or("", String::as_str)
    }
}

/// The cell a row carries when what it shows was made while the stack ran and is not in the config.
pub const CELL_RUNTIME: &str = "_runtime";
