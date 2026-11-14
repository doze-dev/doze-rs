//! The routes, as one table: what is served (Confluent's routes, then doze's own under `/_doze/`), matched segment by
//! segment. A path that matches a route under another method is 405, any other 404, both in the registry's JSON.

/// A served route: its method, its pattern (`{name}` is one path segment), and which handler answers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteDef {
    pub method: &'static str,
    pub pattern: &'static str,
    pub route: Route,
}

/// What a request asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Route {
    Root,
    Subjects,
    Versions,
    Register,
    Lookup,
    DeleteSubject,
    Version,
    VersionSchema,
    ReferencedBy,
    DeleteVersion,
    Schemas,
    Types,
    SchemaById,
    SchemaText,
    IdSubjects,
    IdVersions,
    CompatVersion,
    CompatLevel,
    GetConfig,
    GetSubjectConfig,
    PutConfig,
    PutSubjectConfig,
    DeleteConfig,
    DeleteSubjectConfig,
    GetMode,
    GetSubjectMode,
    PutMode,
    PutSubjectMode,
    DeleteSubjectMode,
    DozeOverview,
    DozeRequests,
    DozeVersion,
    DozeResolved,
    DozeCheck,
}

const fn def(method: &'static str, pattern: &'static str, route: Route) -> RouteDef {
    RouteDef {
        method,
        pattern,
        route,
    }
}

/// Every route served, in the order the ledger lists them (docs/port/behaviours/registry.md).
pub const ROUTES: &[RouteDef] = &[
    def("GET", "/", Route::Root),
    def("GET", "/subjects", Route::Subjects),
    def("GET", "/subjects/{subject}/versions", Route::Versions),
    def("POST", "/subjects/{subject}/versions", Route::Register),
    def("POST", "/subjects/{subject}", Route::Lookup),
    def("DELETE", "/subjects/{subject}", Route::DeleteSubject),
    def(
        "GET",
        "/subjects/{subject}/versions/{version}",
        Route::Version,
    ),
    def(
        "GET",
        "/subjects/{subject}/versions/{version}/schema",
        Route::VersionSchema,
    ),
    def(
        "GET",
        "/subjects/{subject}/versions/{version}/referencedby",
        Route::ReferencedBy,
    ),
    def(
        "DELETE",
        "/subjects/{subject}/versions/{version}",
        Route::DeleteVersion,
    ),
    def("GET", "/schemas", Route::Schemas),
    def("GET", "/schemas/types", Route::Types),
    def("GET", "/schemas/ids/{id}", Route::SchemaById),
    def("GET", "/schemas/ids/{id}/schema", Route::SchemaText),
    def("GET", "/schemas/ids/{id}/subjects", Route::IdSubjects),
    def("GET", "/schemas/ids/{id}/versions", Route::IdVersions),
    def(
        "POST",
        "/compatibility/subjects/{subject}/versions/{version}",
        Route::CompatVersion,
    ),
    def(
        "POST",
        "/compatibility/subjects/{subject}/versions",
        Route::CompatLevel,
    ),
    def("GET", "/config", Route::GetConfig),
    def("GET", "/config/{subject}", Route::GetSubjectConfig),
    def("PUT", "/config", Route::PutConfig),
    def("PUT", "/config/{subject}", Route::PutSubjectConfig),
    def("DELETE", "/config", Route::DeleteConfig),
    def("DELETE", "/config/{subject}", Route::DeleteSubjectConfig),
    def("GET", "/mode", Route::GetMode),
    def("GET", "/mode/{subject}", Route::GetSubjectMode),
    def("PUT", "/mode", Route::PutMode),
    def("PUT", "/mode/{subject}", Route::PutSubjectMode),
    def("DELETE", "/mode/{subject}", Route::DeleteSubjectMode),
    def("GET", "/_doze/overview", Route::DozeOverview),
    def("GET", "/_doze/requests", Route::DozeRequests),
    def(
        "GET",
        "/_doze/subjects/{subject}/versions/{version}",
        Route::DozeVersion,
    ),
    def(
        "GET",
        "/_doze/subjects/{subject}/versions/{version}/resolved",
        Route::DozeResolved,
    ),
    def("POST", "/_doze/subjects/{subject}/check", Route::DozeCheck),
];

/// The path values a matched route captured.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Captures {
    pub subject: Option<String>,
    pub version: Option<String>,
    pub id: Option<String>,
}

/// Why no route answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Miss {
    NotFound,
    MethodNotAllowed,
}

fn matches(pattern: &str, segs: &[String]) -> Option<Captures> {
    let parts: Vec<&str> = pattern
        .strip_prefix('/')
        .unwrap_or(pattern)
        .split('/')
        .collect();
    if parts.len() != segs.len() {
        return None;
    }
    let mut c = Captures::default();
    for (part, seg) in parts.iter().zip(segs) {
        match *part {
            "{subject}" => c.subject = Some(seg.clone()),
            "{version}" => c.version = Some(seg.clone()),
            "{id}" => c.id = Some(seg.clone()),
            lit if lit == seg => {}
            _ => return None,
        }
    }
    Some(c)
}

/// The route a method and path (already split into unescaped segments) ask for. HEAD is answered as GET.
///
/// # Errors
/// No route has the path, or none has it under this method.
pub fn find(method: &str, segs: &[String]) -> Result<(Route, Captures), Miss> {
    let method = if method == "HEAD" { "GET" } else { method };
    let mut other_method = false;
    for d in ROUTES {
        if let Some(c) = matches(d.pattern, segs) {
            if d.method == method {
                return Ok((d.route, c));
            }
            other_method = true;
        }
    }
    Err(if other_method {
        Miss::MethodNotAllowed
    } else {
        Miss::NotFound
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::text::segments;

    fn at(method: &str, path: &str) -> Result<(Route, Captures), Miss> {
        find(method, &segments(path).unwrap())
    }

    #[test]
    fn routes_match_by_segment_and_method() {
        assert_eq!(at("GET", "/").unwrap().0, Route::Root);
        let (r, c) = at("GET", "/subjects/a%2Fb/versions/latest").unwrap();
        assert_eq!(
            (r, c.subject.as_deref(), c.version.as_deref()),
            (Route::Version, Some("a/b"), Some("latest"))
        );
        assert_eq!(at("HEAD", "/subjects").unwrap().0, Route::Subjects);
        assert_eq!(at("GET", "/schemas/types").unwrap().0, Route::Types);
        assert_eq!(at("GET", "/subjects/").unwrap_err(), Miss::NotFound);
        assert_eq!(
            at("PATCH", "/subjects").unwrap_err(),
            Miss::MethodNotAllowed
        );
        assert_eq!(at("DELETE", "/mode").unwrap_err(), Miss::MethodNotAllowed);
        assert_eq!(at("GET", "/contexts").unwrap_err(), Miss::NotFound);
    }
}
