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
    pub client: String,
    pub api: String,
    pub topic: String,
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
            client: String::new(),
            api: String::new(),
            topic: String::new(),
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
    pub id: u64,
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
    pub refused: Vec<WireRefusal>,
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
    pub clients_ids: Vec<String>,
    pub topics_seen: Vec<String>,
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

/// One connected client.
#[derive(Debug, Clone, PartialEq)]
pub struct ClientRow {
    pub client: String,
    pub tag: Option<String>,
    pub software: Option<String>,
    /// The address of its newest connection, and how many more it has.
    pub peer: Option<String>,
    pub more: usize,
    /// The APIs it has used, with the highest version of each, most used first.
    pub apis: Vec<(&'static str, i16)>,
    pub principal: String,
    pub last_ms: i64,
    pub requests: u64,
    pub errors: u64,
}

/// The clients drawer.
#[derive(Debug, Clone, PartialEq)]
pub struct ClientsView {
    pub rows: Vec<ClientRow>,
    pub connections: usize,
    pub now_ms: i64,
}

/// Home's strip: who has spoken.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SpokeView {
    /// Distinct client ids seen since the broker started (those the ring still knows), and requests in the window.
    pub apps: usize,
    pub requests: u64,
    pub oldest_ms: Option<i64>,
    pub last_ms: Option<i64>,
    /// How long ago the last request landed, in milliseconds.
    pub ago_ms: Option<i64>,
    pub last_client: Option<String>,
}

/// The first-request page: the newest client the broker had not heard from before.
#[derive(Debug, Clone, PartialEq)]
pub struct FirstView {
    pub client: String,
    pub software: Option<String>,
    pub peer: Option<String>,
    pub first_ms: i64,
    /// Its requests in order, oldest first, up to a dozen.
    pub rows: Vec<WireRow>,
    /// Whether every request of it was answered without an error.
    pub all_answered: bool,
    pub refused: usize,
    pub topics_listed: Option<u32>,
    pub profile: String,
    pub listen: String,
    pub versions: Vec<(&'static str, i16)>,
}

/// The longest a filter's text is read.
const MOST_TEXT: usize = 128;

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
            client: text("client"),
            api: text("api"),
            topic: text("topic"),
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
        crate::form::encode_query(&[
            ("q", &self.text),
            ("client", &self.client),
            ("api", &self.api),
            ("topic", &self.topic),
            ("group", &self.group),
            ("kind", &self.kind),
            ("refused", if self.refused_only { "1" } else { "" }),
            ("idle", if self.hide_idle { "" } else { "0" }),
            ("before", &before),
            ("limit", &limit),
        ])
    }

    /// The same filters with one field changed (a click on a client, an API or a topic), and the page back dropped.
    #[must_use]
    pub fn with(&self, field: &str, value: &str) -> Self {
        let mut q = Self {
            before: None,
            ..self.clone()
        };
        match field {
            "client" => q.client = value.to_owned(),
            "api" => q.api = value.to_owned(),
            "topic" => q.topic = value.to_owned(),
            "group" => q.group = value.to_owned(),
            "kind" => q.kind = value.to_owned(),
            _ => {}
        }
        q
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(s: &str) -> WireQuery {
        WireQuery::parse(&crate::form::parse(s))
    }

    #[test]
    fn an_address_is_the_filters_and_back_again() {
        let w = q(
            "q=ord&client=admin-cli&api=Produce&topic=orders&refused=1&idle=0&before=12&limit=50",
        );
        assert_eq!(
            (
                w.text.as_str(),
                w.client.as_str(),
                w.api.as_str(),
                w.topic.as_str()
            ),
            ("ord", "admin-cli", "Produce", "orders")
        );
        assert!(w.refused_only && !w.hide_idle);
        assert_eq!((w.before, w.limit), (Some(12), 50));
        assert_eq!(q(&w.to_query()[1..]), w);
        assert_eq!(WireQuery::default().to_query(), "");
    }

    #[test]
    fn nonsense_is_no_filter_and_the_limit_is_held_to_its_range() {
        assert_eq!(q("before=abc&until=-1&limit=999999").limit, WIRE_MOST);
        assert_eq!(q("limit=0").limit, 1);
        assert_eq!(q("before=abc").before, None);
        assert!(q("idle=yes").hide_idle);
        assert_eq!(q(&format!("q={}", "x".repeat(1000))).text.len(), 128);
    }

    #[test]
    fn a_click_on_a_cell_filters_by_it_and_leaves_the_page_back() {
        let w = q("q=a&before=9").with("client", "orders");
        assert_eq!(
            (w.client.as_str(), w.before, w.text.as_str()),
            ("orders", None, "a")
        );
        assert_eq!(w.with("nothing", "x"), w);
    }
}

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
