//! Route selection, by AWS's precedence rather than first match: a literal segment beats a path variable, a variable beats a greedy
//! `{proxy+}`, and among greedy matches the deepest wins. An HTTP API's exact method beats `ANY`, and `$default` takes what nothing
//! else does. A REST resource without the method (or `ANY`) is no match: AWS answers it as it answers an unknown path.

use serde_json::{Map, Value};

use super::Route;
use crate::doc::{obj, text};

/// How a template segment matches a request segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Seg {
    Greedy,
    Var,
    Literal,
}

fn kind(part: &str) -> Seg {
    if part.starts_with('{') && part.ends_with("+}") {
        Seg::Greedy
    } else if part.starts_with('{') && part.ends_with('}') {
        Seg::Var
    } else {
        Seg::Literal
    }
}

fn var_name(part: &str) -> &str {
    part.trim_start_matches('{')
        .trim_end_matches('}')
        .trim_end_matches('+')
}

/// How a template matches `path`: the variables it binds, and its rank (higher is more specific), or `None`.
fn fit(template: &[&str], path: &[String]) -> Option<(Vec<(String, String)>, Vec<Seg>)> {
    let mut params = Vec::new();
    let mut rank = Vec::new();
    for (i, part) in template.iter().enumerate() {
        match kind(part) {
            Seg::Greedy => {
                let rest = path.get(i..).filter(|r| !r.is_empty())?;
                params.push((var_name(part).to_owned(), rest.join("/")));
                rank.push(Seg::Greedy);
                return Some((params, rank));
            }
            Seg::Var => {
                params.push((var_name(part).to_owned(), path.get(i)?.clone()));
                rank.push(Seg::Var);
            }
            Seg::Literal => {
                if path.get(i)? != part {
                    return None;
                }
                rank.push(Seg::Literal);
            }
        }
    }
    (template.len() == path.len()).then_some((params, rank))
}

/// The REST method for `path` and `method` in a deployment's snapshot.
pub(crate) fn rest(snap: &Value, path: &[String], method: &str) -> Option<Route> {
    let resources = obj(snap, "resources")?;
    let mut best: Option<(Vec<Seg>, &Value, Vec<(String, String)>)> = None;
    for res in resources.values() {
        let template = text(res, "path").unwrap_or("/");
        let parts: Vec<&str> = template.split('/').filter(|s| !s.is_empty()).collect();
        let Some((params, rank)) = fit(&parts, path) else {
            continue;
        };
        if best.as_ref().is_none_or(|(r, _, _)| rank > *r) {
            best = Some((rank, res, params));
        }
    }
    let (_, res, params) = best?;
    let methods = obj(res, "resourceMethods")?;
    let m = methods.get(method).or_else(|| methods.get("ANY"))?;
    Some(Route {
        resource_id: text(res, "id").unwrap_or_default().to_owned(),
        resource_path: text(res, "path").unwrap_or("/").to_owned(),
        params,
        integration: m.get("methodIntegration").cloned().unwrap_or(Value::Null),
        method: m.clone(),
        authorizer: None,
        validator: None,
        responses: None,
        integration_responses: None,
    })
}

/// The HTTP API route for `path` and `method`.
pub(crate) fn http(config: &Value, path: &[String], method: &str) -> Option<Route> {
    let routes = obj(config, "routes")?;
    let mut best: Option<((Vec<Seg>, bool), &Value, Vec<(String, String)>)> = None;
    let mut fallback = None;
    for route in routes.values() {
        let key = text(route, "routeKey").unwrap_or_default();
        if key == "$default" {
            fallback = Some(route);
            continue;
        }
        let Some((m, template)) = key.split_once(' ') else {
            continue;
        };
        let exact = m == method;
        if !exact && m != "ANY" {
            continue;
        }
        let parts: Vec<&str> = template.split('/').filter(|s| !s.is_empty()).collect();
        let Some((params, rank)) = fit(&parts, path) else {
            continue;
        };
        let score = (rank, exact);
        if best.as_ref().is_none_or(|(s, _, _)| score > *s) {
            best = Some((score, route, params));
        }
    }
    let (route, params) = match best {
        Some((_, r, p)) => (r, p),
        None => (fallback?, Vec::new()),
    };
    Some(route_of(route, params))
}

/// The WebSocket route for a selected route key, or `$default`.
pub(crate) fn websocket(config: &Value, key: &str) -> Option<Route> {
    let routes = obj(config, "routes")?;
    let by = |k: &str| routes.values().find(|r| text(r, "routeKey") == Some(k));
    let route = by(key).or_else(|| {
        (!key.starts_with('$') || key == "$default")
            .then(|| by("$default"))
            .flatten()
    })?;
    Some(route_of(route, Vec::new()))
}

fn route_of(route: &Value, params: Vec<(String, String)>) -> Route {
    let key = text(route, "routeKey").unwrap_or_default();
    Route {
        resource_id: text(route, "routeId").unwrap_or_default().to_owned(),
        resource_path: key.to_owned(),
        params,
        method: route.clone(),
        integration: Value::Null,
        authorizer: None,
        validator: None,
        responses: None,
        integration_responses: None,
    }
}

/// The map of a route's path variables, for an event.
pub(crate) fn params_map(params: &[(String, String)]) -> Map<String, Value> {
    params
        .iter()
        .map(|(k, v)| (k.clone(), Value::String(v.clone())))
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn segs(p: &str) -> Vec<String> {
        p.split('/')
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn literal_beats_variable_beats_greedy_and_the_deepest_greedy_wins() {
        let snap = json!({"resources": {
            "a": {"id": "a", "path": "/users/{id}", "resourceMethods": {"GET": {}}},
            "b": {"id": "b", "path": "/users/me", "resourceMethods": {"GET": {}}},
            "c": {"id": "c", "path": "/{proxy+}", "resourceMethods": {"ANY": {}}},
            "d": {"id": "d", "path": "/api/{proxy+}", "resourceMethods": {"ANY": {}}},
            "e": {"id": "e", "path": "/"}
        }});
        let pick = |p: &str, m: &str| rest(&snap, &segs(p), m).map(|r| r.resource_id);
        assert_eq!(pick("/users/me", "GET").as_deref(), Some("b"));
        assert_eq!(pick("/users/42", "GET").as_deref(), Some("a"));
        assert_eq!(pick("/api/x/y", "POST").as_deref(), Some("d"));
        assert_eq!(pick("/other/x", "GET").as_deref(), Some("c"));
        // The root has no method: no match.
        assert_eq!(pick("/", "GET"), None);
        let r = rest(&snap, &segs("/api/x/y"), "GET").unwrap();
        assert_eq!(r.params, vec![("proxy".to_owned(), "x/y".to_owned())]);
    }

    #[test]
    fn an_http_api_prefers_the_exact_method_and_falls_back_to_default() {
        let config = json!({"routes": {
            "1": {"routeId": "1", "routeKey": "ANY /orders/{id}"},
            "2": {"routeId": "2", "routeKey": "GET /orders/{id}"},
            "3": {"routeId": "3", "routeKey": "$default"}
        }});
        let pick = |p: &str, m: &str| http(&config, &segs(p), m).map(|r| r.resource_id);
        assert_eq!(pick("/orders/1", "GET").as_deref(), Some("2"));
        assert_eq!(pick("/orders/1", "PUT").as_deref(), Some("1"));
        assert_eq!(pick("/elsewhere", "GET").as_deref(), Some("3"));
    }
}
