//! Finding the operation of a REST request: the model's `@http` bindings are the routing table.
//!
//! A route is a method, a path template (`/{Bucket}/{Key+}`), literal query parameters (`?uploads`, `?list-type=2`), and the
//! members of the input that are required and travel in the query or a header (`uploadId`, `x-amz-copy-source`): those tell
//! `PutObject` from `CopyObject` and `GetObject` from `ListParts`, as the service itself does. Of the routes a request fits, the one
//! that names the most literals wins. `x-id` is how an SDK tags its own operation and says nothing to a server.

use aws_schema::tables::{OpRef, Tables};

/// A path segment of a template.
#[derive(Debug)]
enum Seg {
    Lit(String),
    /// `{Bucket}`: one segment, bound to input member `.0`.
    Label(u32),
    /// `{Key+}`: the rest of the path, which may hold slashes.
    Greedy(u32),
}

#[derive(Debug)]
struct RouteDef {
    op: u32,
    method: String,
    segs: Vec<Seg>,
    /// Literal query parameters and the value they must have, if the template gives one.
    lits: Vec<(String, Option<String>)>,
    need_query: Vec<String>,
    need_header: Vec<String>,
    score: usize,
}

/// The routes of one model.
#[derive(Debug)]
pub struct Router {
    routes: Vec<RouteDef>,
}

/// The operation a request is for, and the values its path gave.
#[derive(Debug, PartialEq, Eq)]
pub struct Matched {
    /// The operation: an index into the tables.
    pub op: u32,
    /// Each path label as (input member index, decoded value).
    pub labels: Vec<(u32, String)>,
}

impl Router {
    /// The routes of every operation of `tables` that has an `@http` binding. Built on first use: a table of routes is not carried
    /// by a process that never takes a REST request.
    #[must_use]
    pub fn new(tables: &Tables) -> Self {
        let routes = (0..tables.n_ops())
            .filter_map(|i| tables.op(i).map(|o| (i, o)))
            .filter(|(_, o)| !o.uri.is_empty())
            .map(|(i, o)| route_of(tables, i, &o))
            .collect();
        Self { routes }
    }

    /// The operation for `method` and `path` (as sent, still percent-encoded) with these decoded query pairs and header names.
    #[must_use]
    pub fn find(
        &self,
        method: &str,
        path: &str,
        query: &[(String, String)],
        has_header: impl Fn(&str) -> bool,
    ) -> Option<Matched> {
        let raw: Vec<&str> = path.trim_start_matches('/').split('/').collect();
        let mut best: Option<(usize, Matched)> = None;
        for r in self.routes.iter().filter(|r| r.method == method) {
            let fits = r.lits.iter().all(|(k, v)| {
                query
                    .iter()
                    .any(|(qk, qv)| qk == k && v.as_ref().is_none_or(|v| v == qv))
            }) && r.need_query.iter().all(|k| query.iter().any(|(qk, _)| qk == k))
                && r.need_header.iter().all(|h| has_header(h));
            if !fits || best.as_ref().is_some_and(|(s, _)| *s >= r.score) {
                continue;
            }
            if let Some(labels) = r.labels(&raw) {
                best = Some((
                    r.score,
                    Matched {
                        op: r.op,
                        labels,
                    },
                ));
            }
        }
        best.map(|(_, m)| m)
    }
}

impl RouteDef {
    /// The path labels, if `raw` (the path split at slashes) fits the template.
    fn labels(&self, raw: &[&str]) -> Option<Vec<(u32, String)>> {
        let mut out = Vec::new();
        let mut at = 0;
        for seg in &self.segs {
            match seg {
                Seg::Lit(lit) => {
                    if raw.get(at).copied() != Some(lit.as_str()) {
                        return None;
                    }
                    at += 1;
                }
                Seg::Label(member) => {
                    let part = raw.get(at).copied().filter(|p| !p.is_empty())?;
                    out.push((*member, decode(part)?));
                    at += 1;
                }
                Seg::Greedy(member) => {
                    let rest = raw.get(at..).filter(|r| r.iter().any(|p| !p.is_empty()))?;
                    let parts: Option<Vec<String>> = rest.iter().map(|p| decode(p)).collect();
                    out.push((*member, parts?.join("/")));
                    return Some(out);
                }
            }
        }
        // `/bucket/` is `/bucket`.
        let tail = raw.get(at..).unwrap_or_default();
        (tail.is_empty() || tail == [""]).then_some(out)
    }
}

fn decode(part: &str) -> Option<String> {
    aws_core::uri::decode_str(part).map(std::borrow::Cow::into_owned)
}

fn route_of(tables: &Tables, op: u32, o: &OpRef<'_>) -> RouteDef {
    let (path, query) = o.uri.split_once('?').unwrap_or((o.uri, ""));
    let labels = input_members(tables, o);
    let label_of = |name: &str| {
        labels
            .iter()
            .find(|(n, _, _)| n == name)
            .map_or(0, |(_, ix, _)| *ix)
    };
    let segs: Vec<Seg> = path
        .trim_start_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .map(|s| match s.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
            Some(name) => name.strip_suffix('+').map_or_else(
                || Seg::Label(label_of(name)),
                |greedy| Seg::Greedy(label_of(greedy)),
            ),
            None => Seg::Lit(s.to_owned()),
        })
        .collect();
    let lits: Vec<(String, Option<String>)> = query
        .split('&')
        .filter(|p| !p.is_empty() && !p.starts_with("x-id"))
        .map(|p| match p.split_once('=') {
            Some((k, v)) => (k.to_owned(), Some(v.to_owned())),
            None => (p.to_owned(), None),
        })
        .collect();
    let need = |bind: u8| -> Vec<String> {
        labels
            .iter()
            .filter(|(_, _, m)| m.0 == bind && m.1)
            .filter_map(|(_, _, m)| m.2.clone())
            .collect()
    };
    let literal_segs = segs.iter().filter(|s| matches!(s, Seg::Lit(_))).count();
    let need_header: Vec<String> = need(4).iter().map(|h| h.to_ascii_lowercase()).collect();
    let need_query = need(2);
    let score = literal_segs * 100 + (lits.len() + need_query.len() + need_header.len()) * 10;
    RouteDef {
        op,
        method: o.method.to_owned(),
        segs,
        lits,
        need_query,
        need_header,
        score,
    }
}

/// (member name, member index, (binding, required, binding name)) of each member of the operation's input.
type InputMember = (String, u32, (u8, bool, Option<String>));

fn input_members(tables: &Tables, o: &OpRef<'_>) -> Vec<InputMember> {
    let Some(shape) = o.input.and_then(|s| tables.shape(s)) else {
        return Vec::new();
    };
    (shape.first..shape.first + shape.count)
        .filter_map(|i| tables.member(i).map(|m| (i, m)))
        .map(|(i, m)| {
            (
                m.name.to_owned(),
                i,
                (
                    m.bind,
                    m.flags & aws_schema::tables::F_REQUIRED != 0,
                    m.bind_str.map(str::to_owned),
                ),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use aws_model_s3::tables::TABLES;

    use super::*;

    fn op(m: &Matched) -> &str {
        TABLES.op(m.op).map_or("", |o| o.name)
    }

    fn find(method: &str, path: &str, query: &str, headers: &[&str]) -> Option<String> {
        let q: Vec<(String, String)> = query
            .split('&')
            .filter(|p| !p.is_empty())
            .map(|p| {
                let (k, v) = p.split_once('=').unwrap_or((p, ""));
                (k.to_owned(), v.to_owned())
            })
            .collect();
        let router = Router::new(&TABLES);
        router
            .find(method, path, &q, |h| headers.contains(&h))
            .map(|m| op(&m).to_owned())
    }

    #[test]
    fn s3_routes_by_literals_and_required_members() {
        let cases: &[(&str, &str, &str, &[&str], &str)] = &[
            ("GET", "/", "", &[], "ListBuckets"),
            ("PUT", "/b", "", &[], "CreateBucket"),
            ("PUT", "/b/", "", &[], "CreateBucket"),
            ("GET", "/b", "", &[], "ListObjects"),
            ("GET", "/b", "list-type=2", &[], "ListObjectsV2"),
            ("GET", "/b", "versions", &[], "ListObjectVersions"),
            ("GET", "/b/a/b/c", "x-id=GetObject", &[], "GetObject"),
            ("GET", "/b/k", "tagging", &[], "GetObjectTagging"),
            ("GET", "/b/k", "uploadId=u", &[], "ListParts"),
            ("PUT", "/b/k", "", &[], "PutObject"),
            ("PUT", "/b/k", "", &["x-amz-copy-source"], "CopyObject"),
            ("PUT", "/b/k", "partNumber=1&uploadId=u", &[], "UploadPart"),
            ("POST", "/b/k", "uploads", &[], "CreateMultipartUpload"),
            ("POST", "/b/k", "uploadId=u", &[], "CompleteMultipartUpload"),
            ("DELETE", "/b/k", "uploadId=u", &[], "AbortMultipartUpload"),
            ("DELETE", "/b/k", "", &[], "DeleteObject"),
            ("POST", "/b", "delete", &[], "DeleteObjects"),
            ("HEAD", "/b/k", "", &[], "HeadObject"),
            ("HEAD", "/b", "", &[], "HeadBucket"),
            ("PUT", "/b", "cors", &[], "PutBucketCors"),
        ];
        for (method, path, query, headers, want) in cases {
            assert_eq!(
                find(method, path, query, headers).as_deref(),
                Some(*want),
                "{method} {path}?{query}"
            );
        }
        assert_eq!(find("PATCH", "/b", "", &[]), None);
    }

    #[test]
    fn labels_are_decoded_and_the_key_keeps_its_slashes() {
        let router = Router::new(&TABLES);
        let m = router
            .find("GET", "/b/a%20b/c%2Fd/", &[], |_| false)
            .unwrap();
        let values: Vec<&str> = m.labels.iter().map(|(_, v)| v.as_str()).collect();
        assert_eq!(values, ["b", "a b/c/d/"]);
    }
}
