//! The router, built from the route table and nothing else, with the layers in front of it in the order the plan says:
//! security headers, then the host guard, then the same-origin proof, then the body limit.

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::middleware::{from_fn, from_fn_with_state};
use axum::routing::{MethodRouter, get, post};

use crate::guard::{MAX_BODY, body_limit, host_guard, same_origin};
use crate::handlers;
use crate::headers::security_headers;
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

/// The console's router.
pub fn router(state: Shared) -> Router {
    let mut routes: Router<Shared> = Router::new();
    for route in ROUTES {
        debug_assert!(matches!(route.method, Method::Get | Method::Post));
        for path in paths(&state.base, route.path) {
            routes = routes.route(&path, method_router(route));
        }
    }
    routes
        .fallback(handlers::not_found)
        .method_not_allowed_fallback(handlers::method_not_allowed)
        .layer(DefaultBodyLimit::max(MAX_BODY))
        .layer(from_fn(body_limit))
        .layer(from_fn(same_origin))
        .layer(from_fn_with_state(state.clone(), host_guard))
        .layer(from_fn(security_headers))
        .with_state(state)
}
