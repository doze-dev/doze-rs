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

/// The handler of a row. The match has no wildcard: a handler that is not wired does not compile.
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
        (Handler::Fragment, _) => get(handlers::fragment),
        (Handler::Events, _) => get(handlers::events),
        (Handler::Static, _) => get(handlers::static_file),
        (Handler::Health, _) => get(handlers::health),
        (Handler::Planned(title), _) => get(
            move |axum::extract::State(s): axum::extract::State<Shared>| async move {
                handlers::planned(s, title).await
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
