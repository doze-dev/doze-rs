//! What S3 answers without a signature: a browser's CORS preflight, a page of a bucket's website, and the lifecycle hook.
//!
//! **Addresses.** A website is reached as AWS names it, `<bucket>.s3-website-<region>.amazonaws.com` or
//! `<bucket>.s3-website.<region>.amazonaws.com`, and locally as `<bucket>.s3-website.localhost:4566` (any host whose label after the
//! bucket is `s3-website` or starts with `s3-website-` or `s3-website.`); for a client that cannot set `Host`, the path form
//! `/_aws/s3-website/<bucket>/<key>` serves the same pages (the doze path convention of `/_aws/lambda-url/`). A preflight is any `OPTIONS`
//! no other service owns, path-style or virtual-hosted. `POST /_aws/s3/lifecycle/<region>` applies a region's lifecycle rules now and
//! answers `{"Actions": n, "Armed": bool}` (a doze extension, for tests that cannot wait for midnight).
//!
//! **Region.** These requests are not signed for a region. The region in the host is asked first when there is one, then the home
//! region, then every region with S3 state on disk, for the first that has the bucket.
//!
//! A website is served to anyone, as an unsigned request to a trusting gateway is (S3's own rule wants a public-read policy); its
//! error pages are AWS's HTML ones (wording from memory).

use std::sync::Arc;

use aws_wire::service::{WebReply, WebRequest};
use aws_wire::{ApiError, RequestId};
use bytes::Bytes;
use serde_json::{Value, json};

use crate::cors::{self, Asked};
use crate::req::{Ask, Out};
use crate::{HOME_REGION, Inner};

const WEBSITE_PATH: &str = "/_aws/s3-website/";
const SWEEP_PATH: &str = "/_aws/s3/lifecycle/";

/// The bucket a website host names, and the region in it if there is one.
fn website_host(host: &str) -> Option<(String, Option<String>)> {
    let name = host.rsplit_once(':').map_or(host, |(h, p)| {
        if p.bytes().all(|b| b.is_ascii_digit()) {
            h
        } else {
            host
        }
    });
    let labels: Vec<&str> = name.split('.').collect();
    let at = labels
        .iter()
        .position(|l| *l == "s3-website" || l.starts_with("s3-website-"))
        .filter(|at| *at > 0)?;
    let bucket = labels.get(..at)?.join(".");
    let label = labels.get(at)?;
    let region = match label.strip_prefix("s3-website-") {
        Some(r) => Some(r.to_owned()),
        None => labels
            .get(at + 1)
            .filter(|r| r.contains('-'))
            .map(|r| (*r).to_owned()),
    };
    Some((bucket, region))
}

/// The bucket and object of a path-style or virtual-hosted request (for a preflight), and the region the host names.
fn rest_target(host: &str, path: &str) -> (String, String, Option<String>) {
    let name = host.split(':').next().unwrap_or_default();
    let labels: Vec<&str> = name.split('.').collect();
    let at = labels
        .iter()
        .position(|l| *l == "s3" || l.starts_with("s3-"))
        .filter(|at| *at > 0 && name.parse::<std::net::IpAddr>().is_err());
    let region_after = |i: usize| {
        labels
            .get(i + 1)
            .filter(|r| r.contains('-'))
            .map(|r| (*r).to_owned())
    };
    let trimmed = path.trim_start_matches('/');
    match at {
        Some(at) => (
            labels.get(..at).map(|l| l.join(".")).unwrap_or_default(),
            trimmed.to_owned(),
            region_after(at),
        ),
        None => {
            let (bucket, key) = trimmed.split_once('/').unwrap_or((trimmed, ""));
            let region = labels
                .iter()
                .position(|l| *l == "s3")
                .and_then(region_after);
            (bucket.to_owned(), key.to_owned(), region)
        }
    }
}

pub(crate) fn owns(host: &str, path: &str) -> bool {
    path.starts_with(WEBSITE_PATH) || path.starts_with(SWEEP_PATH) || website_host(host).is_some()
}

pub(crate) fn owns_preflight(path: &str) -> bool {
    // Another service's private address is not a bucket.
    !path.starts_with("/_aws/")
}

fn header<'a>(req: &'a WebRequest, name: &str) -> Option<&'a str> {
    req.headers
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.as_str())
}

fn decode(s: &str) -> String {
    aws_core::uri::decode_str(s).map_or_else(|| s.to_owned(), std::borrow::Cow::into_owned)
}

/// S3's XML error document.
fn xml_error(e: &ApiError) -> WebReply {
    let id = RequestId::new();
    let r = aws_wire::rest::response::error(e, &id, false);
    WebReply {
        status: r.status,
        headers: with_ids(r.headers, &id),
        body: Bytes::from(r.body),
    }
}

fn with_ids(mut headers: Vec<(String, String)>, id: &RequestId) -> Vec<(String, String)> {
    headers.push(("x-amz-request-id".to_owned(), id.as_str().to_owned()));
    headers.push((
        "x-amz-id-2".to_owned(),
        aws_wire::rest::response::host_id(id),
    ));
    headers.push(("server".to_owned(), "AmazonS3".to_owned()));
    headers
}

/// The HTML page a website answers an error with (memory).
fn html_error(e: &ApiError) -> WebReply {
    let id = RequestId::new();
    let reason = match e.status {
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Error",
    };
    let mut items = format!(
        "<li>Code: {}</li>\n<li>Message: {}</li>\n",
        e.code, e.message
    );
    for (k, v) in &e.extra {
        items.push_str(&format!("<li>{k}: {}</li>\n", escape(v)));
    }
    let body = format!(
        "<html>\n<head><title>{s} {reason}</title></head>\n<body>\n<h1>{s} {reason}</h1>\n<ul>\n{items}<li>RequestId: {id}</li>\n<li>HostId: {host}</li>\n</ul>\n<hr/>\n</body>\n</html>\n",
        s = e.status,
        id = id.as_str(),
        host = aws_wire::rest::response::host_id(&id),
    );
    WebReply {
        status: e.status,
        headers: with_ids(
            vec![(
                "content-type".to_owned(),
                "text/html; charset=utf-8".to_owned(),
            )],
            &id,
        ),
        body: Bytes::from(body),
    }
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

impl Inner {
    /// The regions to look for a bucket in, the host's first.
    fn regions(&self, hint: Option<&str>) -> Vec<String> {
        let mut out = Vec::new();
        let exists = |r: &str| r == HOME_REGION || self.dir.join("regions").join(r).is_dir();
        if let Some(h) = hint.filter(|h| exists(h)) {
            out.push(h.to_owned());
        }
        if !out.iter().any(|r| r == HOME_REGION) {
            out.push(HOME_REGION.to_owned());
        }
        for e in std::fs::read_dir(self.dir.join("regions"))
            .into_iter()
            .flatten()
            .flatten()
        {
            if let Some(name) = e.file_name().to_str()
                && !out.iter().any(|r| r == name)
            {
                out.push(name.to_owned());
            }
        }
        out
    }

    /// The region that has `bucket`, and its CORS and website configurations.
    async fn locate(&self, hint: Option<&str>, bucket: &str) -> Option<(String, Value)> {
        for region in self.regions(hint) {
            if let Ok(out) = self.ask(&region, Ask::Configs(bucket.to_owned())).await {
                return Some((region, out.json));
            }
        }
        None
    }

    /// The CORS headers of an actual request to `bucket` in `region`.
    pub(crate) async fn cors_headers(
        &self,
        region: &str,
        bucket: &str,
        origin: &str,
        method: &str,
    ) -> Vec<(String, String)> {
        let Ok(out) = self.ask(region, Ask::Configs(bucket.to_owned())).await else {
            return Vec::new();
        };
        cors::actual(out.json.get("cors").and_then(Value::as_str), origin, method)
    }

    pub(crate) async fn serve_web(self: &Arc<Self>, req: WebRequest) -> WebReply {
        if let Some(region) = req.path.strip_prefix(SWEEP_PATH) {
            return self.sweep_hook(&req, region).await;
        }
        if let Some(rest) = req.path.strip_prefix(WEBSITE_PATH) {
            let (bucket, key) = rest.split_once('/').unwrap_or((rest, ""));
            return self
                .page(&req, bucket, key, None, &format!("{WEBSITE_PATH}{bucket}/"))
                .await;
        }
        if let Some((bucket, region)) = website_host(&req.host) {
            let key = req.path.trim_start_matches('/').to_owned();
            return self.page(&req, &bucket, &key, region.as_deref(), "/").await;
        }
        self.preflight(&req).await
    }

    async fn preflight(&self, req: &WebRequest) -> WebReply {
        let (bucket, key, region) = rest_target(&req.host, &req.path);
        let found = if bucket.is_empty() {
            None
        } else {
            self.locate(region.as_deref(), &bucket).await
        };
        let Some((_, configs)) = found else {
            // (memory) a preflight for a bucket that is not there.
            return xml_error(
                &ApiError::new(404, "NoSuchBucket", "The specified bucket does not exist")
                    .with("BucketName", bucket.as_str()),
            );
        };
        let asked = Asked {
            origin: header(req, "origin"),
            method: header(req, "access-control-request-method"),
            headers: header(req, "access-control-request-headers"),
            object: !key.is_empty(),
        };
        match cors::preflight(configs.get("cors").and_then(Value::as_str), &asked) {
            Ok(headers) => WebReply {
                status: 200,
                headers: with_ids(headers, &RequestId::new()),
                body: Bytes::new(),
            },
            Err(e) => xml_error(&e),
        }
    }

    /// A page of a website: `root` is the path its pages are under, for the redirects.
    async fn page(
        &self,
        req: &WebRequest,
        bucket: &str,
        key: &str,
        hint: Option<&str>,
        root: &str,
    ) -> WebReply {
        let Some((region, configs)) = self.locate(hint, bucket).await else {
            return html_error(
                &ApiError::new(404, "NoSuchBucket", "The specified bucket does not exist")
                    .with("BucketName", bucket),
            );
        };
        if req.method == "OPTIONS" {
            return self.preflight(req).await;
        }
        let head = req.method == "HEAD";
        if !head && req.method != "GET" {
            // (memory)
            return html_error(
                &ApiError::new(
                    405,
                    "MethodNotAllowed",
                    "The specified method is not allowed against this resource.",
                )
                .with("Method", req.method.as_str())
                .with("ResourceType", "OBJECT"),
            );
        }
        let answer = self
            .ask(&region, Ask::Website(bucket.to_owned(), decode(key)))
            .await;
        let mut reply = match answer {
            Ok(out) => page_reply(out, root, head).await,
            Err(e) => html_error(&e),
        };
        if let Some(origin) = header(req, "origin") {
            reply.headers.extend(cors::actual(
                configs.get("cors").and_then(Value::as_str),
                origin,
                &req.method,
            ));
        }
        reply
    }

    async fn sweep_hook(&self, req: &WebRequest, region: &str) -> WebReply {
        let known = self.regions(None);
        if req.method != "POST" || !known.iter().any(|r| r == region) {
            return xml_error(&ApiError::new(
                404,
                "NoSuchRegion",
                "POST /_aws/s3/lifecycle/<region> sweeps a region that has S3 state.",
            ));
        }
        match self.sweep_now(region).await {
            Ok(swept) => WebReply {
                status: 200,
                headers: vec![("content-type".to_owned(), "application/json".to_owned())],
                body: Bytes::from(
                    json!({"Actions": swept.actions, "Armed": swept.armed}).to_string(),
                ),
            },
            Err(e) => xml_error(&e),
        }
    }
}

/// The HTTP answer for a page the actor found.
async fn page_reply(out: Out, root: &str, head: bool) -> WebReply {
    let text = |k: &str| out.json.get(k).and_then(Value::as_str).map(str::to_owned);
    if let Some(location) = text("Location") {
        let location = if location.contains("://") {
            location
        } else {
            format!("{root}{}", location.trim_start_matches('/'))
        };
        return WebReply {
            status: out.status.unwrap_or(302),
            headers: with_ids(vec![("location".to_owned(), location)], &RequestId::new()),
            body: Bytes::new(),
        };
    }
    let mut headers = Vec::new();
    for (member, name) in [
        ("ContentType", "content-type"),
        ("ETag", "etag"),
        ("CacheControl", "cache-control"),
        ("ContentEncoding", "content-encoding"),
        ("ContentDisposition", "content-disposition"),
        ("ContentLanguage", "content-language"),
    ] {
        if let Some(v) = text(member) {
            headers.push((name.to_owned(), v));
        }
    }
    if let Some(t) = out.json.get("LastModified").and_then(Value::as_i64) {
        headers.push((
            "last-modified".to_owned(),
            aws_core::time::format_http_date(t),
        ));
    }
    let mut body = Vec::new();
    if !head && let Some(slices) = out.body {
        let mut stream = crate::download::stream(slices);
        while let Some(Ok(chunk)) = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await {
            body.extend_from_slice(&chunk);
        }
    }
    WebReply {
        status: out.status.unwrap_or(200),
        headers: with_ids(headers, &RequestId::new()),
        body: Bytes::from(body),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn website_hosts_name_the_bucket_and_maybe_the_region() {
        assert_eq!(
            website_host("site.example.s3-website-us-west-2.amazonaws.com"),
            Some(("site.example".into(), Some("us-west-2".into())))
        );
        assert_eq!(
            website_host("b.s3-website.eu-west-1.amazonaws.com"),
            Some(("b".into(), Some("eu-west-1".into())))
        );
        assert_eq!(
            website_host("b.s3-website.localhost:4566"),
            Some(("b".into(), None))
        );
        assert_eq!(website_host("b.s3.localhost:4566"), None);
        assert_eq!(website_host("s3-website.localhost"), None);
        assert!(owns("127.0.0.1:4566", "/_aws/s3-website/b/index.html"));
        assert!(!owns("127.0.0.1:4566", "/b/index.html"));
    }

    #[test]
    fn a_preflight_names_its_bucket_by_path_or_host() {
        assert_eq!(
            rest_target("127.0.0.1:4566", "/b/a/k.txt"),
            ("b".into(), "a/k.txt".into(), None)
        );
        assert_eq!(
            rest_target("b.s3.us-west-2.amazonaws.com", "/k"),
            ("b".into(), "k".into(), Some("us-west-2".into()))
        );
        assert_eq!(
            rest_target("s3.eu-west-1.amazonaws.com", "/b"),
            ("b".into(), String::new(), Some("eu-west-1".into()))
        );
        assert!(owns_preflight("/b/k"));
        assert!(!owns_preflight("/_aws/lambda-url/x"));
    }
}
