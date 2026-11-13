//! One request at the front door: refused unless this machine sent it, redirected to https when it names a declared
//! domain and a doze process holds the https port, answered with a list of what is fronted when the name has no
//! route, else proxied to the name's backend with its Host kept (doze-aws builds the URLs it returns from it: a queue
//! made through aws.doze must report an aws.doze URL) and upgrades carried (a dev server's hot reload is a WebSocket).

use crate::Registry;
use crate::domains::{lookup_domain, valid_domain};
use crate::front::INGRESS_TLS_KEY;
use bytes::Bytes;
use http::header::{CONNECTION, HOST, LOCATION, UPGRADE};
use http::{HeaderValue, Request, Response, StatusCode};
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper_util::rt::TokioIo;
use std::net::IpAddr;
use tokio::net::TcpStream;

pub(crate) type Body = BoxBody<Bytes, hyper::Error>;

/// Headers that belong to one hop, never forwarded (RFC 9110 7.6.1), as Go's ReverseProxy drops them.
const HOP: &[&str] = &[
    "connection",
    "keep-alive",
    "proxy-connection",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
];

fn text(status: StatusCode, body: String) -> Response<Body> {
    let mut r = Response::new(
        Full::new(Bytes::from(body))
            .map_err(|never| match never {})
            .boxed(),
    );
    *r.status_mut() = status;
    r.headers_mut().insert(
        http::header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    r
}

fn host_only(host: &str) -> &str {
    match host.rsplit_once(':') {
        Some((h, p)) if p.parse::<u16>().is_ok() => h,
        _ => host,
    }
}

/// The names fronted here, for the 404 body: the usual cause is a service that is not running.
fn routed(reg: &Registry) -> String {
    let lines: Vec<String> = reg
        .snapshot()
        .into_iter()
        .filter(|(h, e)| !h.starts_with('_') && !e.target.is_empty())
        .map(|(h, e)| format!("  http://{h}  ->  {}  ({})", e.target, e.owner))
        .collect();
    if lines.is_empty() {
        "  (nothing: no doze service has registered a route)".into()
    } else {
        lines.join("\n")
    }
}

/// Answers one request that arrived from `peer` over the plain port (`tls` false) or the https port.
pub(crate) async fn handle(
    reg: Registry,
    req: Request<Incoming>,
    peer: IpAddr,
    tls: bool,
) -> Response<Body> {
    // Before the route lookup, so a refused peer learns nothing about what is registered.
    if !crate::on_this_machine(peer) {
        return text(
            StatusCode::FORBIDDEN,
            "doze names are served to this machine only\n".into(),
        );
    }
    let host = req
        .headers()
        .get(HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let name = crate::normalize(host_only(&host));
    if !tls && !crate::in_zone(&name) && valid_domain(&name).is_ok() {
        let snap = reg.snapshot();
        if lookup_domain(&snap, &name).is_some() && snap.contains_key(INGRESS_TLS_KEY) {
            let path = req.uri().path_and_query().map_or("/", |p| p.as_str());
            let mut r = text(StatusCode::TEMPORARY_REDIRECT, String::new());
            if let Ok(v) = HeaderValue::from_str(&format!("https://{name}{path}")) {
                r.headers_mut().insert(LOCATION, v);
            }
            return r;
        }
    }
    let Some(target) = reg.front_target(&host) else {
        let body = format!(
            "no doze service is fronting {name}\n\nfronted here:\n{}\n",
            routed(&reg)
        );
        return text(StatusCode::NOT_FOUND, body);
    };
    match forward(req, &target, &host, peer, tls).await {
        Ok(r) => r,
        Err(e) => text(
            StatusCode::BAD_GATEWAY,
            format!("{name} is routed to {target}, which did not answer: {e}\n"),
        ),
    }
}

async fn forward(
    mut req: Request<Incoming>,
    target: &str,
    host: &str,
    peer: IpAddr,
    tls: bool,
) -> Result<Response<Body>, String> {
    let upgrade = req.headers().get(UPGRADE).cloned();
    let client_side = upgrade.is_some().then(|| hyper::upgrade::on(&mut req));
    let stream = TcpStream::connect(target)
        .await
        .map_err(|e| e.to_string())?;
    let (mut sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .map_err(|e| e.to_string())?;
    tokio::spawn(async move {
        let _ = conn.with_upgrades().await;
    });
    let (mut parts, body) = req.into_parts();
    for h in HOP {
        parts.headers.remove(*h);
    }
    if let Some(u) = &upgrade {
        parts.headers.insert(UPGRADE, u.clone());
        parts
            .headers
            .insert(CONNECTION, HeaderValue::from_static("upgrade"));
    }
    let forwarded = [
        ("x-forwarded-for", peer.to_string()),
        ("x-forwarded-host", host.to_string()),
        (
            "x-forwarded-proto",
            if tls { "https".into() } else { "http".into() },
        ),
    ];
    for (k, v) in forwarded {
        if let Ok(v) = HeaderValue::from_str(&v) {
            parts.headers.insert(k, v);
        }
    }
    parts.uri = parts
        .uri
        .path_and_query()
        .map_or("/", |p| p.as_str())
        .parse()
        .map_err(|e| format!("{e}"))?;
    let mut resp = sender
        .send_request(Request::from_parts(parts, body))
        .await
        .map_err(|e| e.to_string())?;
    if resp.status() == StatusCode::SWITCHING_PROTOCOLS
        && let Some(client_side) = client_side
    {
        let backend_side = hyper::upgrade::on(&mut resp);
        tokio::spawn(async move {
            if let (Ok(c), Ok(b)) = (client_side.await, backend_side.await) {
                let _ =
                    tokio::io::copy_bidirectional(&mut TokioIo::new(c), &mut TokioIo::new(b)).await;
            }
        });
    }
    Ok(resp.map(BodyExt::boxed))
}
