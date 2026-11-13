//! The JSON API of the React console (`docs/port/console/react-plan.md`): the same view-models the maud pages render,
//! serialised, under `/api/v1/`.
//!
//! Every answer is JSON. A refusal is `{ "code": "...", "message": "..." }` with Kafka's own words where a Kafka error is the
//! cause. Every request must carry `X-Console: 1` (the console's fetch client sets it): a page on another site cannot send a
//! custom header without a CORS preflight, which this server never grants. A request that is not a plain read must also pass
//! the same-origin proof of [`crate::origin`]. The TypeScript types of every answer are generated from these Rust types by a
//! test (`types::the_typescript_types_of_the_api_are_current`), so the two cannot drift.
//!
//! How to add an endpoint: a response type here (derive `serde::Serialize` and, under `cfg(test)`, `ts_rs::TS`), a handler,
//! a line in [`routes`], a line in the type list of `api/types.rs`, then `cargo test -p kafka-console api::types` writes the
//! TypeScript file and `bun run build` in `web/` rebuilds the bundle.

use axum::Router;
use axum::extract::{RawQuery, Request, State};
use axum::http::header::{CONTENT_TYPE, HOST, ORIGIN};
use axum::http::{HeaderName, HeaderValue, StatusCode};
use axum::middleware::{Next, from_fn};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde::Serialize;

use crate::model::{ConnectView, HomeState, HomeView, ProbeView, Span};
use crate::state::Shared;
use crate::{origin, snippets, words};

pub mod groups;
pub mod live;
pub mod topics;
#[cfg(test)]
mod types;

pub mod access;
pub mod config;
pub mod config_edit;
pub mod lab;
pub mod settings;
pub mod topic_admin;
pub mod topic_writes;

/// The header the console's fetch client sends on every API request.
pub const PROOF_HEADER: &str = "x-console";

const FETCH_SITE: HeaderName = HeaderName::from_static("sec-fetch-site");

/// A refusal: a stable code a script can match and the words a person reads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ApiError {
    /// `FORBIDDEN`, `NOT_FOUND`, `METHOD_NOT_ALLOWED`, `BAD_REQUEST`, or Kafka's own error name (`TOPIC_AUTHORIZATION_FAILED`).
    pub code: String,
    pub message: String,
}

/// A value as a JSON answer. (axum's own `Json` needs its `json` feature, which brings a package the build does not have;
/// this is the three lines it would be.)
#[must_use]
pub fn json<T: Serialize>(status: StatusCode, value: &T) -> Response {
    match serde_json::to_vec(value) {
        Ok(body) => {
            let mut res = (status, body).into_response();
            res.headers_mut().insert(
                CONTENT_TYPE,
                HeaderValue::from_static("application/json; charset=utf-8"),
            );
            res
        }
        Err(e) => {
            tracing::warn!(error = %e, "an API answer could not be serialised");
            (StatusCode::INTERNAL_SERVER_ERROR, "serialisation failed").into_response()
        }
    }
}

/// A refusal as a response.
#[must_use]
pub fn error(status: StatusCode, code: &str, message: &str) -> Response {
    json(
        status,
        &ApiError {
            code: code.to_owned(),
            message: message.to_owned(),
        },
    )
}

/// The answer to an `/api/` address no route has.
#[must_use]
pub fn not_found() -> Response {
    error(
        StatusCode::NOT_FOUND,
        "NOT_FOUND",
        "The console has no API at this address.",
    )
}

/// The proof every API request must carry (see the module text).
pub async fn guard(req: Request, next: Next) -> Response {
    if let Some(refused) = refusal(&req) {
        return refused;
    }
    next.run(req).await
}

/// Why [`guard`] refuses a request, if it does.
fn refusal(req: &Request) -> Option<Response> {
    let header = |name: &HeaderName| req.headers().get(name).and_then(|v| v.to_str().ok());
    if header(&HeaderName::from_static(PROOF_HEADER)) != Some("1") {
        return Some(error(
            StatusCode::FORBIDDEN,
            "FORBIDDEN",
            "The console's API answers only its own pages, which send the header X-Console: 1.",
        ));
    }
    if let Some(site) = header(&FETCH_SITE)
        && !site.trim().eq_ignore_ascii_case("same-origin")
    {
        return Some(error(
            StatusCode::FORBIDDEN,
            "FORBIDDEN",
            origin::Refusal::CrossSite.words(),
        ));
    }
    origin::check(
        req.method().as_str(),
        header(&ORIGIN),
        header(&FETCH_SITE),
        header(&HOST),
    )
    .err()
    .map(|why| error(StatusCode::FORBIDDEN, "FORBIDDEN", why.words()))
}

/// The words of Home that Rust already decides (the boards' copy), so the page does not say them a second way.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct HomeWords {
    pub title: String,
    /// The sentence under the title: static (no live number in it, CON-805).
    pub sub: Vec<Span>,
    pub instance_lede: String,
    /// `21 of 64`, or the count alone.
    pub open_partitions: String,
    pub advertised_lede: String,
    /// The callout's headline when a transaction is near its timeout.
    pub attention_headline: Option<String>,
    /// What Home says of the open transactions it did not look at.
    pub unchecked_note: Option<String>,
    /// The command that names a client so it shows as itself.
    pub first_command: String,
}

/// `GET /api/v1/home`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct HomeResponse {
    pub home: HomeView,
    pub words: HomeWords,
}

impl HomeResponse {
    #[must_use]
    pub fn new(home: HomeView) -> Self {
        let words = HomeWords {
            title: words::home_title(&home.state).to_owned(),
            sub: words::home_sub(&home),
            instance_lede: words::instance_lede(&home.instance),
            open_partitions: words::open_partitions(&home.instance),
            advertised_lede: words::home_advertised_lede(&home.probe),
            attention_headline: match &home.state {
                HomeState::Attention(a) => Some(words::attention_headline(a.count, home.unchecked)),
                _ => None,
            },
            unchecked_note: words::unchecked_note(home.unchecked),
            first_command: snippets::first_command(&home.listen),
        };
        Self { home, words }
    }
}

/// The words of Connect that Rust decides.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ConnectWords {
    pub advertised_lede: Vec<Span>,
    /// `bootstrap.servers=...` and `client.id=my-app`: what "Copy as client.properties" copies.
    pub properties: String,
    /// `KAFKA_BOOTSTRAP_SERVERS=...`.
    pub environment: String,
    /// The recipe for another machine or a container.
    pub recipe: String,
}

/// `GET /api/v1/connect`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ConnectResponse {
    pub connect: ConnectView,
    pub words: ConnectWords,
}

impl ConnectResponse {
    #[must_use]
    pub fn new(connect: ConnectView) -> Self {
        let port = connect
            .listen
            .rsplit_once(':')
            .map_or("9092", |(_, p)| p)
            .to_owned();
        let words = ConnectWords {
            advertised_lede: words::connect_advertised_lede(&connect.probe),
            properties: format!("bootstrap.servers={}\nclient.id=my-app", connect.listen),
            environment: format!("KAFKA_BOOTSTRAP_SERVERS={}", connect.listen),
            recipe: format!(
                "doze-kafka --listen 0.0.0.0:{port} \\\n  --advertised-host 192.168.1.20 \\\n  --advertised-port {port}"
            ),
        };
        Self { connect, words }
    }
}

/// `GET /api/v1/shell`: what the top bar and the sidebar show on every page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ShellView {
    /// The address a client is pointed at, `127.0.0.1:9092`.
    pub address: String,
    /// `Kafka 4 profile`.
    pub profile: String,
    /// The security chip: `Open`, `Open · ACLs`, `SASL SCRAM + TLS`.
    pub security: String,
    /// Whether the chip carries a lock (something is on) rather than the open dot.
    pub locked: bool,
    /// The console is served beyond loopback with no login (`--console-unprotected`).
    pub unprotected: bool,
    /// The sidebar's counts.
    pub topics: usize,
    pub groups: usize,
    /// Where the server-rendered console still lives until slice R5 (`/classic/`).
    pub classic: String,
}

/// `GET /api/v1/jump?q=`: the palette's data. Pages are the client's own list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct JumpView {
    pub query: String,
    pub topics: Vec<String>,
    pub groups: Vec<String>,
    /// How many matched before the lists were cut at [`JUMP_MOST`] each.
    pub total: usize,
}

/// The most names of each kind the palette is sent.
pub const JUMP_MOST: usize = 200;

async fn home(State(s): State<Shared>) -> Response {
    json(
        StatusCode::OK,
        &HomeResponse::new(s.source.home(None).await),
    )
}

async fn connect(State(s): State<Shared>) -> Response {
    json(
        StatusCode::OK,
        &ConnectResponse::new(s.source.connect().await),
    )
}

/// `POST /api/v1/probe`: run the advertised-listener check now.
async fn probe(State(s): State<Shared>) -> Response {
    json::<ProbeView>(StatusCode::OK, &s.source.probe(true).await)
}

async fn shell(State(s): State<Shared>) -> Response {
    let chrome = s.source.chrome().await;
    let topics = s.source.topics("", false, None).await.total;
    let groups = s.source.group_names().await.len();
    json(
        StatusCode::OK,
        &ShellView {
            address: chrome.address,
            profile: chrome.profile,
            security: chrome.security,
            locked: chrome.locked,
            unprotected: chrome.unprotected,
            topics,
            groups,
            classic: s.base.url("/"),
        },
    )
}

async fn jump(State(s): State<Shared>, RawQuery(query): RawQuery) -> Response {
    let fields = crate::form::parse(query.as_deref().unwrap_or(""));
    let q: String = crate::form::get(&fields, "q")
        .unwrap_or("")
        .trim()
        .chars()
        .take(249)
        .collect();
    let want = q.to_lowercase();
    let topics: Vec<String> = s
        .source
        .topics("", false, None)
        .await
        .rows
        .into_iter()
        .map(|r| r.name)
        .filter(|t| want.is_empty() || t.to_lowercase().contains(&want))
        .collect();
    let groups: Vec<String> = s
        .source
        .group_names()
        .await
        .into_iter()
        .filter(|g| want.is_empty() || g.to_lowercase().contains(&want))
        .collect();
    let total = topics.len() + groups.len();
    json(
        StatusCode::OK,
        &JumpView {
            query: q,
            topics: topics.into_iter().take(JUMP_MOST).collect(),
            groups: groups.into_iter().take(JUMP_MOST).collect(),
            total,
        },
    )
}

/// The API's routes under `web` (the console's root, `--console-prefix`), behind [`guard`].
pub fn routes(web: &crate::base::Base) -> Router<Shared> {
    Router::new()
        .route(&web.url("/api/v1/home"), get(home))
        .route(&web.url("/api/v1/connect"), get(connect))
        .route(&web.url("/api/v1/probe"), post(probe))
        .route(&web.url("/api/v1/shell"), get(shell))
        .route(&web.url("/api/v1/jump"), get(jump))
        .merge(topic_writes::routes(web))
        .merge(topic_admin::routes(web))
        .merge(config::routes(web))
        .merge(config_edit::routes(web))
        .merge(groups::routes(web))
        .route(&web.url("/api/v1/live"), get(live::live))
        .route(&web.url("/api/v1/live/request/{id}"), get(live::request))
        .route(&web.url("/api/v1/live/clients"), get(live::clients))
        .route(&web.url("/api/v1/live/first"), get(live::first))
        .route(&web.url("/api/v1/live/topic-wire"), get(live::topic_wire))
        .route(&web.url("/api/v1/live/group-wire"), get(live::group_wire))
        .route(&web.url("/api/v1/tail/live"), get(live::tail))
        .merge(topics::routes(web))
        .route(&web.url("/api/v1/settings"), get(settings::page))
        .route(
            &web.url("/api/v1/settings/edit"),
            get(settings::edit_form).post(settings::edit),
        )
        .route(&web.url("/api/v1/access"), get(access::page))
        .route(
            &web.url("/api/v1/access/acls/new"),
            get(access::create_form).post(access::create),
        )
        .route(
            &web.url("/api/v1/access/acls/delete"),
            get(access::delete_form).post(access::delete),
        )
        .route(&web.url("/api/v1/lab"), get(lab::home))
        .route(&web.url("/api/v1/lab/form"), get(lab::form))
        .route(&web.url("/api/v1/lab/rules"), post(lab::make))
        .route(&web.url("/api/v1/lab/rules/{id}"), get(lab::rule))
        .route(
            &web.url("/api/v1/lab/rules/{id}/enabled"),
            post(lab::switch),
        )
        .route(&web.url("/api/v1/lab/rules/{id}/remove"), post(lab::remove))
        .route(&web.url("/api/v1/lab/history/clear"), post(lab::clear))
        .route(
            &web.url("/api/v1/lab/once/{kind}"),
            get(lab::once_form).post(lab::once),
        )
        .route(
            &web.url("/api/v1/lab/compact"),
            get(lab::compact_form).post(lab::compact),
        )
        .route_layer(from_fn(guard))
}

/// The address of every GET endpoint, for the tests that read them all (the planted-secret test among them).
pub const GET_PATHS: &[&str] = &[
    "/api/v1/home",
    "/api/v1/connect",
    "/api/v1/shell",
    "/api/v1/jump",
    "/api/v1/jump?q=or",
    // Slice R1b: the topic writes and the Config tab (the worlds of the tests have `orders`).
    "/api/v1/create-topic",
    "/api/v1/create-topic?step=review&name=fresh&partitions=2",
    "/api/v1/topics/orders/produce",
    "/api/v1/topics/orders/produce?key=k&value=v&hn0=a&hv0=b",
    "/api/v1/topics/orders/add-partitions",
    "/api/v1/topics/orders/delete-records?partition=0",
    "/api/v1/topics/orders/delete",
    "/api/v1/topics/orders/config",
    "/api/v1/topics/orders/config/edit?key=retention.ms",
    "/api/v1/topics/orders/config/edit?key=cleanup.policy",
    "/api/v1/topics/orders/config/edit?key=min.compaction.lag.ms",
    "/api/v1/topics/orders/config/reset?key=retention.ms",
    // Groups (R2): the addresses that answer whatever groups the broker has (those of one group are
    // `groups::GROUP_GET_PATHS`, over the fixtures' groups).
    "/api/v1/groups",
    "/api/v1/groups?q=pay&state=stable&state=empty",
    // Live (R3). The tail (`/api/v1/tail/live`) is a stream that never ends: its own tests read it.
    "/api/v1/live",
    "/api/v1/live?refused=1&idle=0&client=orders&api=Produce&topic=orders",
    "/api/v1/live/clients",
    "/api/v1/live/first",
    "/api/v1/live/topic-wire?topic=orders",
    "/api/v1/live/group-wire?group=fulfilment",
    // R1a: the Topics pages. Messages and a record are not here: showing a record's key, value and headers is their job.
    "/api/v1/topics",
    "/api/v1/topics?q=or&internal=1",
    "/api/v1/topics/orders",
    "/api/v1/topics/orders/consumers",
    "/api/v1/topics/orders/log",
    "/api/v1/topics/orders/producers",
    "/api/v1/settings",
    "/api/v1/settings/edit?key=log.retention.ms",
    "/api/v1/access",
    "/api/v1/access?state=secured",
    "/api/v1/access/acls/new?type=topic&name=orders&principal=User:orders",
    "/api/v1/access/acls/delete?type=topic&name=orders&pattern=literal&principal=User:orders&host=*&op=write&perm=allow",
    // The Lab's other GETs answer LAB_OFF where the Lab is not installed (the fixtures, a broker with --lab=false); their tests
    // are in api/lab_tests.rs and the browser gate.
    "/api/v1/lab",
];
