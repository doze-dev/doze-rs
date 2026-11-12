//! The router, built from the route table and nothing else, with the layers in front of it in the order the plan says:
//! security headers, then the host guard, then the same-origin proof, then the body limit.

use axum::Router;
use axum::body::Body;
use axum::extract::DefaultBodyLimit;
use axum::extract::{Request, State};
use axum::http::{Method as HttpMethod, StatusCode};
use axum::middleware::{from_fn, from_fn_with_state};
use axum::response::{IntoResponse, Response};
use axum::routing::{MethodRouter, get, post};
use tower::ServiceExt as _;

use crate::guard::{MAX_BODY, body_limit, host_guard, same_origin};
use crate::handlers;
use crate::headers::security_headers;
use crate::region;
use crate::routes::{Handler, Method, ROUTES, Route, paths};
use crate::state::Shared;

/// The handler of a row. The match has no wildcard: a handler that is not wired does not compile. It is a table, one line a
/// row, so it is as long as the console has handlers.
#[allow(clippy::too_many_lines)]
fn method_router(route: &Route) -> MethodRouter<Shared> {
    match (route.handler, route.method) {
        (Handler::Home, _) => get(handlers::home),
        (Handler::Connect, _) => get(handlers::connect),
        (Handler::Advertised, _) => get(handlers::advertised),
        (Handler::Check, _) => post(handlers::check),
        (Handler::Topics, _) => get(handlers::topics),
        (Handler::Topic, _) => get(handlers::topic),
        (Handler::TopicMessages, _) => get(handlers::topic_messages),
        (Handler::TopicRecord, _) => get(handlers::topic_record),
        (Handler::TopicPartitions, _) => get(handlers::topic_partitions),
        (Handler::TopicConsumers, _) => get(handlers::topic_consumers),
        (Handler::Fragment, _) => get(handlers::fragment),
        (Handler::Live, _) => get(handlers::live),
        (Handler::LiveClients, _) => get(handlers::live_clients),
        (Handler::LiveRequest, _) => get(handlers::live_request),
        (Handler::FirstRequest, _) => get(handlers::first_request),
        (Handler::TopicWire, _) => get(handlers::topic_wire),
        (Handler::GroupWire, _) => get(handlers::group_wire),
        (Handler::Events, _) => get(handlers::events),
        (Handler::Static, _) => get(handlers::static_file),
        (Handler::Health, _) => get(handlers::health),
        (Handler::CreateForm, _) => get(handlers::create_form),
        (Handler::Create, _) => post(handlers::create),
        (Handler::ProduceForm, _) => get(handlers::produce_form),
        (Handler::Produce, _) => post(handlers::produce),
        (Handler::PartitionsAddForm, _) => get(handlers::partitions_add_form),
        (Handler::PartitionsAdd, _) => post(handlers::partitions_add),
        (Handler::DeleteRecordsForm, _) => get(handlers::delete_records_form),
        (Handler::DeleteRecords, _) => post(handlers::delete_records),
        (Handler::DeleteForm, _) => get(handlers::delete_form),
        (Handler::Delete, _) => post(handlers::delete),
        (Handler::Groups, _) => get(handlers::groups_list),
        (Handler::Group, _) => get(handlers::group_page),
        (Handler::GroupMembers, _) => get(handlers::group_members),
        (Handler::GroupMember, _) => get(handlers::group_member),
        (Handler::GroupLag, _) => get(handlers::group_lag),
        (Handler::Story, _) => get(handlers::group_story),
        (Handler::StoryFlapping, _) => get(handlers::story_flapping),
        (Handler::StoryRebalance, _) => get(handlers::story_rebalance),
        (Handler::ResetForm, _) => get(handlers::group_reset_form),
        (Handler::Reset, _) => post(handlers::group_reset),
        (Handler::GroupDeleteForm, _) => get(handlers::group_delete_form),
        (Handler::GroupDelete, _) => post(handlers::group_delete),
        (Handler::OffsetsDeleteForm, _) => get(handlers::offsets_delete_form),
        (Handler::OffsetsDelete, _) => post(handlers::offsets_delete),
        (Handler::ConfigPage, _) => get(handlers::config),
        (Handler::ConfigEditForm, _) => get(handlers::config_edit_form),
        (Handler::ConfigEdit, _) => post(handlers::config_edit),
        (Handler::ConfigResetForm, _) => get(handlers::config_reset_form),
        (Handler::ConfigReset, _) => post(handlers::config_reset),
        (Handler::Jump, _) => get(handlers::jump),
        (Handler::TopicLog, _) => get(handlers::topic_log),
        (Handler::TopicProducers, _) => get(handlers::topic_producers),
        (Handler::TopicTxn, _) => get(handlers::topic_txn),
        (Handler::TopicTxnHeld, _) => get(handlers::topic_txn_held),
        (Handler::Settings, _) => get(handlers::settings),
        (Handler::SettingsEditForm, _) => get(handlers::settings_edit_form),
        (Handler::SettingsEdit, _) => post(handlers::settings_edit),
        (Handler::Access, _) => get(handlers::access),
        (Handler::AclCreateForm, _) => get(handlers::acl_create_form),
        (Handler::AclCreate, _) => post(handlers::acl_create),
        (Handler::AclDeleteForm, _) => get(handlers::acl_delete_form),
        (Handler::AclDelete, _) => post(handlers::acl_delete),
        (Handler::Planned(title), _) => get(
            move |axum::extract::State(s): axum::extract::State<Shared>,
                  params: axum::extract::RawPathParams,
                  headers: axum::http::HeaderMap,
                  uri: axum::http::Uri| async move {
                handlers::unbuilt(s, title, params, headers, uri.path()).await
            },
        ),
    }
}

/// The whole console at `state.web` (`--console-prefix`): the React console (its API, its files, and `index.html` for every
/// other GET, so a deep link works) and the classic console under `state.base` (`{prefix}/classic`) until slice R5. The
/// security headers, the host guard and the body limit stand in front of all of it; the API has its own proof
/// (`api::guard`), the classic console keeps every layer of [`router`].
pub fn console(state: Shared) -> Router {
    let classic = router(state.clone());
    let web = state.web.clone();
    crate::api::routes(&web)
        .route(&web.url("/healthz"), get(handlers::health))
        .fallback(move |s: State<Shared>, req: Request| dispatch(s, classic.clone(), req))
        .layer(DefaultBodyLimit::max(MAX_BODY))
        .layer(from_fn(body_limit))
        .layer(from_fn_with_state(state.clone(), host_guard))
        .layer(from_fn(security_headers))
        .with_state(state)
}

/// Where a request no API route took goes: the classic console, an API 404, a file of the bundle, or `index.html`.
async fn dispatch(State(s): State<Shared>, classic: Router, req: Request<Body>) -> Response {
    let path = req.uri().path().to_owned();
    let Some(rest) = s.web.strip(&path) else {
        return classic.oneshot(req).await.into_response();
    };
    let under = |p: &str| rest == p || rest.starts_with(&format!("{p}/"));
    if under("/classic") {
        return classic.oneshot(req).await.into_response();
    }
    if under("/api") {
        return crate::api::not_found();
    }
    if req.method() != HttpMethod::GET && req.method() != HttpMethod::HEAD {
        return crate::api::error(
            StatusCode::METHOD_NOT_ALLOWED,
            "METHOD_NOT_ALLOWED",
            "This address only answers GET.",
        );
    }
    let file = rest.trim_start_matches('/');
    if let Some(res) = s.bundle.file(file, req.headers()) {
        return res;
    }
    if under("/assets") {
        return (StatusCode::NOT_FOUND, "no such file\n").into_response();
    }
    s.bundle.index()
}

/// The classic console's router.
pub fn router(state: Shared) -> Router {
    let mut routes: Router<Shared> = Router::new();
    for route in ROUTES {
        debug_assert!(matches!(route.method, Method::Get | Method::Post));
        for path in paths(&state.base, route.path) {
            routes = routes.route(&path, method_router(route));
        }
    }
    // The component gallery exists only where the fixtures do (the examples build): a release build has no such address.
    #[cfg(feature = "examples")]
    {
        routes = routes.route(&state.base.url("/gallery"), get(handlers::gallery));
    }
    routes
        .fallback(handlers::not_found)
        .method_not_allowed_fallback(handlers::method_not_allowed)
        .layer(DefaultBodyLimit::max(MAX_BODY))
        .layer(from_fn(body_limit))
        .layer(from_fn(same_origin))
        .layer(from_fn_with_state(state.clone(), host_guard))
        .layer(from_fn(region::layer))
        .layer(from_fn(security_headers))
        .with_state(state)
}
