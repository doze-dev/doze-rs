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
    /// The Consumers tab of a topic: the groups that read it, with their lag on its partitions.
    TopicConsumers,
    Fragment,
    /// Slice 9e: Live, its drawers, the first-request page and the wire pages of a topic and a group.
    Live,
    LiveClients,
    LiveRequest,
    FirstRequest,
    TopicWire,
    GroupWire,
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
    /// Slice 9c, groups: the list, a group, its members and a member, its lag.
    Groups,
    Group,
    GroupMembers,
    GroupMember,
    GroupLag,
    /// Slice 9f: the Story of a group, the flapping rule held against it, and one rebalance as a drawer over the Story.
    Story,
    StoryFlapping,
    StoryRebalance,
    /// The dialogs on a group, and the POST of each.
    ResetForm,
    Reset,
    GroupDeleteForm,
    GroupDelete,
    OffsetsDeleteForm,
    OffsetsDelete,
    /// Slice 9d, the Config tab of a topic: the list, the edit and reset dialogs, and the POST of each.
    ConfigPage,
    ConfigEditForm,
    ConfigEdit,
    ConfigResetForm,
    ConfigReset,
    /// Jump to a topic: the palette to a script, a page of links to a browser.
    Jump,
    /// Slice 9g: the Log page, the Producers page, and the page of a transaction with who it holds back.
    TopicLog,
    TopicProducers,
    TopicTxn,
    TopicTxnHeld,
    /// Slice 9h, Settings: the page, the edit dialog of a dynamic broker config and its POST.
    Settings,
    SettingsEditForm,
    SettingsEdit,
    /// Slice 9h, Access: the page, the Create ACL and Delete ACL dialogs and the POST of each.
    Access,
    AclCreateForm,
    AclCreate,
    AclDeleteForm,
    AclDelete,
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
    get("/jump", &[], Handler::Jump),
    get("/static/{name}", &[], Handler::Static),
    get("/healthz", &[], Handler::Health),
    // The rest of the design: Live is built (9e); what is still not built says so (Lab, a group's story).
    // Slice 9e: the recorder and Live. `/live/clients` is a fixed name and wins over `/live/{id}`.
    get(
        "/live",
        &["live-wire", "live-paused", "live-quiet", "live-load"],
        Handler::Live,
    ),
    get("/live/clients", &["live-clients"], Handler::LiveClients),
    get(
        "/live/{request}",
        &[
            "request-detail",
            "request-refused",
            "request-fenced",
            "request-too-large",
        ],
        Handler::LiveRequest,
    ),
    get(
        "/connect/first-request",
        &["connect-first-request"],
        Handler::FirstRequest,
    ),
    get("/topics/{topic}/wire", &["topic-wire"], Handler::TopicWire),
    get("/groups/{group}/wire", &["group-wire"], Handler::GroupWire),
    // Slice 9b, the topic reads (p9b-read).
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
    get("/topics/{topic}/consumers", &[], Handler::TopicConsumers),
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
    // Slice 9g, the Log and Producers pages (p9g).
    get(
        "/topics/{topic}/producers",
        &["topic-producers"],
        Handler::TopicProducers,
    ),
    get(
        "/topics/{topic}/transactions/{txn}",
        &["txn-open"],
        Handler::TopicTxn,
    ),
    get(
        "/topics/{topic}/transactions/{txn}/held",
        &["txn-holding"],
        Handler::TopicTxnHeld,
    ),
    // Slice 9d, the Config tab (p9d).
    get(
        "/topics/{topic}/config",
        &["topic-config"],
        Handler::ConfigPage,
    ),
    get(
        "/topics/{topic}/config/edit",
        &["config-edit", "config-edit-list", "config-edit-lag"],
        Handler::ConfigEditForm,
    ),
    write(
        "/topics/{topic}/config/edit",
        "IncrementalAlterConfigs",
        &["config-saved", "config-refused", "config-refused-value"],
        Handler::ConfigEdit,
    ),
    get(
        "/topics/{topic}/config/reset",
        &["config-reset"],
        Handler::ConfigResetForm,
    ),
    write(
        "/topics/{topic}/config/reset",
        "IncrementalAlterConfigs",
        &[],
        Handler::ConfigReset,
    ),
    // The Log and Producers tabs of a topic, and the page of a transaction (9g).
    get(
        "/topics/{topic}/log",
        &["topic-log", "log-planner", "log-retention", "log-empty"],
        Handler::TopicLog,
    ),
    // Slice 9c, groups (p9c).
    get("/groups", &["groups-list", "groups-none"], Handler::Groups),
    get(
        "/groups/{group}",
        &["group-detail", "group-detail-848", "group-rebalancing"],
        Handler::Group,
    ),
    get(
        "/groups/{group}/members",
        &["group-members"],
        Handler::GroupMembers,
    ),
    get(
        "/groups/{group}/members/{member}",
        &["member-detail"],
        Handler::GroupMember,
    ),
    get(
        "/groups/{group}/lag",
        &["group-lag", "lag-no-commits", "lag-ahead", "lag-expiring"],
        Handler::GroupLag,
    ),
    get(
        "/groups/{group}/reset",
        &["group-reset-offsets"],
        Handler::ResetForm,
    ),
    write(
        "/groups/{group}/reset",
        "OffsetCommit",
        &["group-reset-refused"],
        Handler::Reset,
    ),
    get(
        "/groups/{group}/delete",
        &["group-delete"],
        Handler::GroupDeleteForm,
    ),
    write(
        "/groups/{group}/delete",
        "DeleteGroups",
        &["group-delete-refused"],
        Handler::GroupDelete,
    ),
    get(
        "/groups/{group}/offsets/delete",
        &["group-offset-delete"],
        Handler::OffsetsDeleteForm,
    ),
    write(
        "/groups/{group}/offsets/delete",
        "OffsetDelete",
        &[],
        Handler::OffsetsDelete,
    ),
    // Slice 9f, the Story (p9f). `/story/flapping` is a fixed name and wins over `/story/{generation}`.
    get("/groups/{group}/story", &["group-story"], Handler::Story),
    get(
        "/groups/{group}/story/flapping",
        &["story-flapping"],
        Handler::StoryFlapping,
    ),
    get(
        "/groups/{group}/story/{generation}",
        &["story-rebalance"],
        Handler::StoryRebalance,
    ),
    get("/lab", &["lab-home"], Handler::Planned("Lab")),
    // Slice 9h, Settings and Access (p9h).
    get("/settings", &["settings-home"], Handler::Settings),
    get(
        "/settings/config/edit",
        &["broker-config-edit"],
        Handler::SettingsEditForm,
    ),
    write(
        "/settings/config/edit",
        "IncrementalAlterConfigs",
        &["broker-config-saved", "broker-config-refused"],
        Handler::SettingsEdit,
    ),
    get(
        "/access",
        &["access-open", "access-secured"],
        Handler::Access,
    ),
    get(
        "/access/acls/new",
        &["access-acl-create"],
        Handler::AclCreateForm,
    ),
    write("/access/acls/new", "CreateAcls", &[], Handler::AclCreate),
    get(
        "/access/acls/delete",
        &["access-acl-delete"],
        Handler::AclDeleteForm,
    ),
    write("/access/acls/delete", "DeleteAcls", &[], Handler::AclDelete),
];

/// Whether `path` is a screen no slice has built yet (its row is `Handler::Planned`): the rail draws it with a "Soon" tag, and
/// the page it opens says so. When a slice builds it the row changes and the tag goes with it.
#[must_use]
pub fn is_planned(path: &str) -> bool {
    ROUTES.iter().any(|r| {
        r.path == path && r.method == Method::Get && matches!(r.handler, Handler::Planned(_))
    })
}

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
