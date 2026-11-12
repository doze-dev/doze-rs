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
    Fragment,
    Events,
    Static,
    Health,
    /// Slice 9b, topic writes: the Create-topic steps and the POST that makes the topic.
    CreateForm,
    Create,
    /// The dialogs on a topic, and the POST of each.
    ProduceForm,
    Produce,
    PartitionsAddForm,
    PartitionsAdd,
    DeleteRecordsForm,
    DeleteRecords,
    DeleteForm,
    Delete,
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

/// A POST that issues the Kafka API `op` and answers the boards `screens`.
const fn write(
    path: &'static str,
    op: &'static str,
    screens: &'static [&'static str],
    handler: Handler,
) -> Route {
    Route {
        path,
        method: Method::Post,
        mutates: true,
        op: Some(op),
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
    get("/topics", &["topics-list"], Handler::Planned("Topics")),
    // Slice 9b, the topic writes (p9b-write).
    get(
        "/topics/new",
        &["topic-create", "topic-create-config", "topic-create-review"],
        Handler::CreateForm,
    ),
    write(
        "/topics/new",
        "CreateTopics",
        &["topic-created", "topic-create-refused"],
        Handler::Create,
    ),
    get(
        "/topics/{topic}/produce",
        &["topic-produce"],
        Handler::ProduceForm,
    ),
    write(
        "/topics/{topic}/produce",
        "Produce",
        &["topic-produce-keyless", "topic-produced"],
        Handler::Produce,
    ),
    get(
        "/topics/{topic}/partitions/add",
        &["topic-add-partitions"],
        Handler::PartitionsAddForm,
    ),
    write(
        "/topics/{topic}/partitions/add",
        "CreatePartitions",
        &["topic-add-partitions-refused"],
        Handler::PartitionsAdd,
    ),
    get(
        "/topics/{topic}/delete-records",
        &["topic-delete-records"],
        Handler::DeleteRecordsForm,
    ),
    write(
        "/topics/{topic}/delete-records",
        "DeleteRecords",
        &["topic-delete-records-refused"],
        Handler::DeleteRecords,
    ),
    get(
        "/topics/{topic}/delete",
        &["topic-delete"],
        Handler::DeleteForm,
    ),
    write(
        "/topics/{topic}/delete",
        "DeleteTopics",
        &["topic-deleted"],
        Handler::Delete,
    ),
    get(
        "/topics/{topic}/producers",
        &["txn-open", "topic-producers"],
        Handler::Planned("Producers"),
    ),
    // The tabs of a topic and the Groups space, for the links of the pages above (built by later slices).
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
