//! What Confluent's web server and framework answer before the registry does (recorded from cp-schema-registry 7.7.1
//! by tools/registry-diff): misses without a `Content-Type`, Jetty's 400 page, OPTIONS, and the type `Accept` chooses.

use super::*;

#[test]
fn the_frameworks_own_answers_are_confluents() {
    let a = plain();
    // Recorded from cp-schema-registry 7.7.1 (tools/registry-diff): the framework's own misses carry no Content-Type,
    // the web server's 400 is its HTML page, and GET /contexts lists the default context.
    let nf = a.serve(&req("GET", "/exporters", ""));
    assert_eq!(
        (nf.status, text(&nf), nf.content_type),
        (
            404,
            "{\"error_code\":404,\"message\":\"HTTP 404 Not Found\"}".into(),
            None
        )
    );
    assert_eq!(text(&a.serve(&req("GET", "/contexts", ""))), "[\".\"]");
    let no = a.serve(&req("DELETE", "/mode", ""));
    assert_eq!((no.status, no.content_type), (405, None));
    let bad = a.serve(&req("GET", "/subjects/%zz/versions", ""));
    assert_eq!(
        (bad.status, text(&bad), bad.content_type),
        (
            400,
            "<h1>Bad Message 400</h1><pre>reason: Bad Request</pre>".into(),
            Some("text/html;charset=iso-8859-1")
        )
    );
    let opts = a.serve(&req("OPTIONS", "/subjects/x/versions", ""));
    assert_eq!(
        (opts.status, text(&opts), opts.allow.as_deref()),
        (
            200,
            "HEAD, POST, GET, OPTIONS".into(),
            Some("HEAD,POST,GET,OPTIONS")
        )
    );
    for (accept, want) in [
        ("application/xml", None),
        ("text/html", None),
        ("application/json", Some("application/json")),
        ("*/*", Some(CONTENT_TYPE)),
        (
            "application/vnd.schemaregistry+json",
            Some("application/vnd.schemaregistry+json"),
        ),
        (
            "application/json;q=0.9, application/vnd.schemaregistry.v1+json;q=0.1",
            Some("application/json"),
        ),
    ] {
        let mut r = req("GET", "/subjects", "");
        r.accept = Some(accept.into());
        let resp = a.serve(&r);
        assert_eq!(resp.content_type, want, "{accept}");
        assert_eq!(
            resp.status,
            if want.is_some() { 200 } else { 406 },
            "{accept}"
        );
    }
}
