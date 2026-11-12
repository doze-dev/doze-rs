//! The route table: the one list the router is built from, the route tests read, and `design/routes.json` is made from.
//!
//! A row is a path, a method, whether it changes anything, the Kafka API behind it, and the screens of the boards it
//! serves. The router has no route that is not a row, and `Handler` is matched without a wildcard, so a row without a
//! handler does not compile. Screens that are states of one route (Home is `home`, `home-quiet` and `home-attention`)
//! are listed together: the data chooses the state, not the address.

/// An HTTP method the console uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
}

/// What answers a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handler {
    Home,
    Connect,
    Advertised,
    Check,
    Topics,
    Topic,
    TopicMessages,
    TopicRecord,
    TopicPartitions,
    Fragment,
    Events,
    Static,
    Health,
    /// A screen of the design that a later slice builds: a page that says so (title).
    Planned(&'static str),
}

/// One route.
#[derive(Debug, Clone, Copy)]
pub struct Route {
    /// An axum path (`/topics/{topic}/producers`).
    pub path: &'static str,
    pub method: Method,
    /// Whether it changes the broker (or only asks it something). Every row that is not a GET needs a same-origin proof
    /// whether it does or not.
    pub mutates: bool,
    /// The Kafka API the boards' `data-op` names, if the route issues one.
    pub op: Option<&'static str>,
    /// The boards (`design/boards/<section>/<id>.html`) this route renders.
    pub screens: &'static [&'static str],
    pub handler: Handler,
}

const fn get(path: &'static str, screens: &'static [&'static str], handler: Handler) -> Route {
    Route {
        path,
        method: Method::Get,
        mutates: false,
        op: None,
        screens,
        handler,
    }
}

/// Every route of the console.
pub const ROUTES: &[Route] = &[
    get(
        "/",
        &["home", "home-quiet", "home-attention"],
        Handler::Home,
    ),
    get("/connect", &["connect"], Handler::Connect),
    get(
        "/connect/advertised",
        &["connect-advertised"],
        Handler::Advertised,
    ),
    Route {
        path: "/connect/advertised/check",
        method: Method::Post,
        mutates: false,
        op: Some("Metadata"),
        screens: &[],
        handler: Handler::Check,
    },
    get("/fragments/{id}", &[], Handler::Fragment),
    get("/events", &[], Handler::Events),
    get("/static/{name}", &[], Handler::Static),
    get("/healthz", &[], Handler::Health),
    // The rest of the design, built by later slices: each says it is not built yet.
    get("/live", &["live-wire"], Handler::Planned("Live")),
    get(
        "/topics",
        &["topics-list", "topics-none", "topics-filtered"],
        Handler::Topics,
    ),
    get(
        "/topics/{topic}",
        &["topic-detail", "topic-offline"],
        Handler::Topic,
    ),
    get(
        "/topics/{topic}/messages",
        &[
            "topic-messages-empty",
            "topic-messages-filtered",
            "topic-readcommitted",
        ],
        Handler::TopicMessages,
    ),
    get(
        "/topics/{topic}/messages/{partition}/{offset}",
        &["topic-record", "topic-record-binary"],
        Handler::TopicRecord,
    ),
    get(
        "/topics/{topic}/partitions",
        &["topic-partitions"],
        Handler::TopicPartitions,
    ),
    // Screens of the same section that other slices (and p9b-write) build: each says it is not built yet. p9b-write replaces
    // the four rows after `/topics/new` with its own handlers when the branches are merged.
    get(
        "/topics/{topic}/produce",
        &["topic-produce", "topic-produce-keyless", "topic-produced"],
        Handler::Planned("Produce"),
    ),
    get(
        "/topics/{topic}/partitions/add",
        &["topic-add-partitions", "topic-add-partitions-refused"],
        Handler::Planned("Add partitions"),
    ),
    get(
        "/topics/{topic}/delete-records",
        &["topic-delete-records", "topic-delete-records-refused"],
        Handler::Planned("Delete records"),
    ),
    get(
        "/topics/{topic}/delete",
        &["topic-delete", "topic-deleted"],
        Handler::Planned("Delete topic"),
    ),
    get(
        "/topics/{topic}/log",
        &["topic-log"],
        Handler::Planned("Log"),
    ),
    get(
        "/topics/{topic}/config",
        &["topic-config"],
        Handler::Planned("Config"),
    ),
    get("/groups", &["groups-list"], Handler::Planned("Groups")),
    get(
        "/topics/new",
        &["topic-create"],
        Handler::Planned("Create a topic"),
    ),
    get(
        "/topics/{topic}/producers",
        &["txn-open", "topic-producers", "txn-holding"],
        Handler::Planned("Producers"),
    ),
    get("/access", &["access-open"], Handler::Planned("Access")),
    get("/lab", &["lab-home"], Handler::Planned("Lab")),
    get(
        "/settings",
        &["settings-home"],
        Handler::Planned("Settings"),
    ),
];

/// The paths a row is registered under for a prefix: the root is `/` and, under a prefix, both `/k` and `/k/`.
#[must_use]
pub fn paths(base: &crate::base::Base, path: &str) -> Vec<String> {
    if path == "/" && !base.prefix().is_empty() {
        vec![base.prefix().to_owned(), format!("{}/", base.prefix())]
    } else {
        vec![base.url(path)]
    }
}

/// `design/routes.json`: each screen id and the pattern of the route that serves it, sorted by id.
#[must_use]
pub fn json() -> String {
    let mut screens: Vec<(&str, &str)> = ROUTES
        .iter()
        .flat_map(|r| r.screens.iter().map(move |s| (*s, r.path)))
        .collect();
    screens.sort_unstable();
    let mut out = String::from("{\n  \"screens\": {\n");
    for (i, (id, path)) in screens.iter().enumerate() {
        let comma = if i + 1 == screens.len() { "" } else { "," };
        out.push_str(&format!("    \"{id}\": \"{path}\"{comma}\n"));
    }
    out.push_str("  }\n}\n");
    out
}

#[cfg(test)]
mod tests;
