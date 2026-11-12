//! What the Live pages say: the tape of requests, one request in full, the clients, and the docks that filter the same tape.
//! Plain data; the recorder makes it, the pages write it. Nothing here is a ring type.

/// How a request ended, as a word and a colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WireClass {
    /// Answered, no error.
    Done,
    /// An error the client handles by itself (a rebalance, a coordinator that moved): amber, "Told to rejoin".
    Told,
    /// The broker failed.
    Failed,
    /// Kafka's own refusal: violet.
    Refused,
    /// An ACL or a login said no: amber, "Denied".
    Denied,
    /// The connection was closed instead of an answer.
    Closed,
}

impl WireClass {
    /// Whether the row is a refusal of either kind (what "Refused only" keeps).
    #[must_use]
    pub fn is_refusal(self) -> bool {
        matches!(
            self,
            Self::Refused | Self::Denied | Self::Closed | Self::Failed
        )
    }
}

/// One row of the tape, for display.
#[derive(Debug, Clone, PartialEq)]
pub struct WireRow {
    pub id: u64,
    /// When the last of its requests began (milliseconds since the epoch), and the first if it stands for several.
    pub at_ms: i64,
    pub since_ms: Option<i64>,
    /// How many requests it stands for: 1, or more when repeats were folded.
    pub count: u32,
    pub client: String,
    /// The client library, as a short tag (`go`, `py`, `java`), when ApiVersions said.
    pub tag: Option<String>,
    pub api: &'static str,
    pub version: i16,
    pub transactional: bool,
    /// The target: topic and partition, the group, the transactional id.
    pub target: String,
    /// A second line under the target (`from 7,402`, `by fulfilment-py-2`, `generation 30`).
    pub target_sub: Option<String>,
    /// The topic of the row, the group, for the docks' filters.
    pub topics: Vec<String>,
    pub group: Option<String>,
    pub class: WireClass,
    pub code: i16,
    pub code_name: String,
    /// What the result adds: `1 record`, `1 batch, 211 B`; for a refusal, the broker's own words.
    pub detail: String,
    pub took_us: u32,
    pub took_sub: Option<String>,
    /// A poll that found nothing (hidden by "Idle polls hidden").
    pub idle: bool,
}

/// What the person asked the tape to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireQuery {
    /// Free text: matches a client, an API, a topic or a group.
    pub text: String,
    /// The apps, APIs and topics to keep (an exact name each): a row passes a kind when it matches ANY value of it, and the
    /// kinds combine with AND. Empty means every one.
    pub client: Vec<String>,
    pub api: Vec<String>,
    pub topic: Vec<String>,
    pub group: String,
    /// A kind of group request: `join`, `commits`, `heartbeats` (the group wire's chips).
    pub kind: String,
    pub refused_only: bool,
    /// Hide the polls that found nothing.
    pub hide_idle: bool,
    /// Only rows older than this id (the next page back).
    pub before: Option<u64>,
    pub limit: usize,
}

/// The rows a page shows at once.
pub const WIRE_LIMIT: usize = 16;
/// The most a person can ask for.
pub const WIRE_MOST: usize = 100;

impl Default for WireQuery {
    fn default() -> Self {
        Self {
            text: String::new(),
            client: Vec::new(),
            api: Vec::new(),
            topic: Vec::new(),
            group: String::new(),
            kind: String::new(),
            refused_only: false,
            hide_idle: true,
            before: None,
            limit: WIRE_LIMIT,
        }
    }
}

/// What the ring covers, computed from what it holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WireWindow {
    /// The rows kept now and the most it keeps.
    pub rows: usize,
    pub capacity: usize,
    /// The oldest row's time: how far back the tape honestly reaches. `None` when it is empty.
    pub oldest_ms: Option<i64>,
    /// The requests those rows stand for.
    pub requests: u64,
}

/// A refusal to link to from the top of the tape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireRefusal {
    /// The newest request refused in this way (the "View" opens it).
    pub id: u64,
    /// How many requests the recorder holds that were refused with this code for this client.
    pub count: u32,
    pub code_name: String,
    pub code: i16,
    pub client: String,
    pub at_ms: i64,
    pub class: WireClass,
}

/// The state of the Live page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveState {
    /// Nothing has reached the broker since it started.
    Quiet,
    /// The tape.
    Steady,
    /// A busy minute: many requests, folded.
    Busy,
}

/// The Live page.
#[derive(Debug, Clone, PartialEq)]
pub struct LiveView {
    pub state: LiveState,
    pub rows: Vec<WireRow>,
    /// Rows that match the filters, and rows kept.
    pub matched: usize,
    pub window: WireWindow,
    pub query: WireQuery,
    /// One entry for each (code, client) that was refused, newest first, at most three.
    pub refused: Vec<WireRefusal>,
    /// How many more (code, client) pairs there are than `refused` shows.
    pub refused_more: usize,
    pub refused_total: usize,
    pub denied_total: usize,
    /// Clients connected now.
    pub clients: usize,
    /// Requests in the last minute.
    pub last_minute: u32,
    /// The most requests one folded row stands for in the last minute (for the busy state's sentence), and the topic.
    pub busiest: Option<(String, u32)>,
    /// Requests recorded since the broker started (folded rows count each request).
    pub seen: u64,
    /// The id of the newest row.
    pub newest_id: u64,
    /// The recorder's version when this was made: where a tail begins (`since`).
    pub cursor: u64,
    /// When this was made.
    pub now_ms: i64,
    /// Where the broker listens and when it started (for the quiet state).
    pub listen: String,
    pub started_ms: Option<i64>,
    /// The id the next page back starts after, if there is one.
    pub older: Option<u64>,
    /// What the tape holds, for the filters' options: each app, API and topic with the rows that would remain if it were chosen.
    pub facets: Facets,
}

/// One option of a filter: a value and how many rows of the tape carry it (among the rows that pass the other filters).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facet {
    pub value: String,
    pub count: usize,
}

/// The options of the Live filters.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Facets {
    pub clients: Vec<Facet>,
    pub apis: Vec<Facet>,
    pub topics: Vec<Facet>,
}

/// One per-partition result of a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WirePart {
    pub topic: String,
    pub partition: i32,
    pub code: i16,
    pub code_name: String,
    pub offset: Option<i64>,
    pub records: u32,
    pub bytes: u64,
}

/// A request in full, as far as the broker kept it (never a request body).
#[derive(Debug, Clone, PartialEq)]
pub struct WireDetail {
    pub row: WireRow,
    pub principal: String,
    pub software: Option<String>,
    pub peer: Option<String>,
    pub correlation: i32,
    pub conn: u64,
    pub request_bytes: u32,
    pub response_bytes: u32,
    pub acks: Option<i16>,
    pub wait_ms: Option<i32>,
    pub isolation: Option<i8>,
    pub from_offset: Option<i64>,
    pub producer: Option<(i64, i16)>,
    pub txn_id: Option<String>,
    pub member: Option<String>,
    pub generation: Option<i32>,
    pub reason: Option<String>,
    /// What the request is about when it names no topic or group.
    pub what: Option<String>,
    /// The broker's own message, if it said one, and the message of a record error.
    pub note: Option<String>,
    pub record_note: Option<String>,
    pub records: u32,
    pub batches: u32,
    pub bytes: u64,
    pub items: u32,
    pub parts: Vec<WirePart>,
    pub parts_more: u32,
    pub topics: Vec<(String, Vec<i32>)>,
    /// The error's own words, a plain sentence, and what would pass (empty where the table has none).
    pub words: String,
    pub plain: String,
    pub pass: String,
    /// The slowest, fastest and median took of a folded row (microseconds).
    pub took_spread: Option<(u32, u32, u32)>,
    /// Requests of the same transactional id or producer that came before (the fenced row's story), oldest first.
    pub story: Vec<WireRow>,
}

/// The longest a filter's text is read.
const MOST_TEXT: usize = 128;
/// The APIs of a repeated `api` field, as the broker names them (`produce` is `Produce`); a name that is not an API is dropped.
fn apis(fields: &[(String, String)]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for v in values(fields, "api") {
        let name = crate::recorder::names::api_of(&v).map(crate::recorder::names::api);
        if let Some(n) = name
            && !out.iter().any(|o| o == n)
        {
            out.push(n.to_owned());
        }
    }
    out
}

/// The most values one kind of filter keeps.
pub const MOST_VALUES: usize = 32;

/// Every value of a repeated field (`topic=a&topic=b`): trimmed, bounded, without empties or repeats, in the order given.
fn values(fields: &[(String, String)], name: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (k, v) in fields {
        let v: String = v.trim().chars().take(MOST_TEXT).collect();
        if k == name && !v.is_empty() && !out.contains(&v) && out.len() < MOST_VALUES {
            out.push(v);
        }
    }
    out
}

impl WireQuery {
    /// The filters of an address (`?q=...&client=...&api=...&topic=...&group=...&refused=1&idle=0&before=12&limit=50`). A
    /// value that cannot be used is left out: a hostile address is a page with no filter, never an error.
    #[must_use]
    pub fn parse(fields: &[(String, String)]) -> Self {
        let text = |name: &str| {
            crate::form::text(fields, name)
                .trim()
                .chars()
                .take(MOST_TEXT)
                .collect::<String>()
        };
        let number =
            |name: &str| crate::form::get(fields, name).and_then(|v| v.trim().parse::<u64>().ok());
        Self {
            text: text("q"),
            client: values(fields, "client"),
            // An API is one the broker has: a name that is not is no filter (it could only ever match nothing).
            api: apis(fields),
            topic: values(fields, "topic"),
            group: text("group"),
            kind: text("kind"),
            refused_only: crate::form::get(fields, "refused") == Some("1"),
            hide_idle: crate::form::get(fields, "idle") != Some("0"),
            before: number("before"),
            limit: number("limit")
                .and_then(|n| usize::try_from(n).ok())
                .map_or(WIRE_LIMIT, |n| n.clamp(1, WIRE_MOST)),
        }
    }

    /// The address's query part for these filters (with its `?`), with `before` as given and the defaults left out.
    #[must_use]
    pub fn to_query(&self) -> String {
        let before = self.before.map(|n| n.to_string()).unwrap_or_default();
        let limit = if self.limit == WIRE_LIMIT {
            String::new()
        } else {
            self.limit.to_string()
        };
        let mut fields: Vec<(&str, &str)> = vec![("q", &self.text)];
        fields.extend(self.client.iter().map(|v| ("client", v.as_str())));
        fields.extend(self.api.iter().map(|v| ("api", v.as_str())));
        fields.extend(self.topic.iter().map(|v| ("topic", v.as_str())));
        fields.extend([
            ("group", self.group.as_str()),
            ("kind", self.kind.as_str()),
            ("refused", if self.refused_only { "1" } else { "" }),
            ("idle", if self.hide_idle { "" } else { "0" }),
            ("before", &before),
            ("limit", &limit),
        ]);
        crate::form::encode_query(&fields)
    }

    /// Whether an app passes the client filter.
    #[must_use]
    pub fn client_ok(&self, client: &str) -> bool {
        self.client.is_empty() || self.client.iter().any(|c| c == client)
    }

    /// Whether an API passes the API filter (the names are the broker's, compared without regard to case).
    #[must_use]
    pub fn api_ok(&self, api: &str) -> bool {
        self.api.is_empty() || self.api.iter().any(|a| a.eq_ignore_ascii_case(api))
    }

    /// Whether a request that names these topics passes the topic filter: it names at least one of the chosen topics, exactly.
    #[must_use]
    pub fn topic_ok<'a>(&self, mut named: impl Iterator<Item = &'a str>) -> bool {
        self.topic.is_empty() || named.any(|n| self.topic.iter().any(|t| t == n))
    }

    /// The same filters without one chosen value (a chip's x), and the page back dropped.
    #[must_use]
    pub fn without(&self, field: &str, value: &str) -> Self {
        let mut q = Self {
            before: None,
            ..self.clone()
        };
        match field {
            "client" => q.client.retain(|v| v != value),
            "api" => q.api.retain(|v| v != value),
            "topic" => q.topic.retain(|v| v != value),
            _ => {}
        }
        q
    }

    /// Whether any filter is on: what an empty tape should say it is empty because of (the page's own state, a pause or a page
    /// back, and the idle toggle's default are not filters).
    #[must_use]
    pub fn is_filtered(&self) -> bool {
        !(self.text.is_empty()
            && self.client.is_empty()
            && self.api.is_empty()
            && self.topic.is_empty()
            && self.group.is_empty()
            && self.kind.is_empty()
            && !self.refused_only)
    }

    /// The same filters with one field changed (a click on a client, an API or a topic), and the page back dropped.
    #[must_use]
    pub fn with(&self, field: &str, value: &str) -> Self {
        let mut q = Self {
            before: None,
            ..self.clone()
        };
        let one = |v: &str| {
            if v.is_empty() {
                Vec::new()
            } else {
                vec![v.to_owned()]
            }
        };
        match field {
            "client" => q.client = one(value),
            "api" => q.api = one(value),
            "topic" => q.topic = one(value),
            "group" => q.group = value.to_owned(),
            "kind" => q.kind = value.to_owned(),
            _ => {}
        }
        q
    }
}

#[cfg(test)]
mod tests;

/// A group's wire: the rows that pass the filters, and the counts the chips say.
#[derive(Debug, Clone, PartialEq)]
pub struct GroupWireView {
    pub group: String,
    pub query: WireQuery,
    pub rows: Vec<WireRow>,
    pub window: WireWindow,
    pub now_ms: i64,
    /// Every request that touched the group among the rows kept, and how many of those were refused.
    pub all: usize,
    pub refused: usize,
}
