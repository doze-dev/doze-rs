//! The region contract: every route answers a whole document, and when the request is htmx's (`HX-Request`) the page's
//! document function answers only the REGION: the document title, `main` (the page's own content, with the breadcrumb), and
//! the two parts of the shell that follow the page (the rail's active item and the overlay root) as out-of-band swaps. The
//! shell around `main` (top bar, rail, scripts) is never sent again, so a client-side navigation moves only the page.
//!
//! Three cases are NOT the region and get the whole document: a request without `HX-Request` (a browser with no script, a reload,
//! a shared link), a history restore (`HX-History-Restore-Request`: htmx puts the body back from a whole document), and a request
//! for an overlay or a fragment, which their own handlers answer (`handlers::overlays`, `x-fragment`).
//!
//! The layer also finishes what only it can: a request that targeted the overlay root (`HX-Target: ovl`, a form of a drawer or
//! a dialog) and got a PAGE back (the result of a write, after the redirect) is told to swap it into `main` instead, to replace
//! the overlay's address in the history by the page's, and to cross-fade; and every answer says it varies on the headers.

use axum::extract::Request;
use axum::http::{HeaderName, HeaderValue};
use axum::middleware::Next;
use axum::response::Response;
use std::sync::{Arc, Mutex};

tokio::task_local! {
    /// Whether the request in flight wants the region.
    static REGION: bool;
    /// The canonical address of the page the request in flight is answering, set by the handler (see [`canonical`]).
    static CANONICAL: Arc<Mutex<Option<String>>>;
}

/// Says what address the page being answered has once its state is read (a filter with its defaults dropped, its duplicates
/// merged). An in-place control sends the form as it stands (`?partition=1&time=&partition=1`); a handler that knows what the
/// state really is calls this, and the layer puts the clean address into the history instead.
pub fn canonical(path_and_query: &str) {
    let _ = CANONICAL.try_with(|c| {
        if let Ok(mut g) = c.lock() {
            *g = Some(path_and_query.to_owned());
        }
    });
}

/// The request header htmx sends on every request it makes.
pub const HX_REQUEST: &str = "hx-request";
/// The header htmx adds when it restores a page from the history (it wants a whole document).
pub const HX_RESTORE: &str = "hx-history-restore-request";
/// The header the preload extension adds to a request it makes ahead of a click.
pub const HX_PRELOADED: &str = "hx-preloaded";
/// The id of the element a request will swap into, as htmx sends it.
pub const HX_TARGET: &str = "hx-target";
/// The id of the overlay root: the target of every overlay a script opens.
pub const OVERLAY_ROOT: &str = "ovl";
/// The id of `main`: the target of every navigation.
pub const MAIN: &str = "main";

/// Whether the request in flight wants only the region (see the module).
#[must_use]
pub fn wanted() -> bool {
    REGION.try_with(|r| *r).unwrap_or(false)
}

/// Runs `fut` as if the request wanted the region (or the whole document). For the tests of the document function.
pub async fn scoped<F: std::future::Future>(want: bool, fut: F) -> F::Output {
    REGION.scope(want, fut).await
}

fn is_true(req: &Request, name: &str) -> bool {
    req.headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.eq_ignore_ascii_case("true"))
}

/// The layer: sets what the request wants for the handlers behind it, and fixes the answer's headers.
pub async fn layer(req: Request, next: Next) -> Response {
    let hx = is_true(&req, HX_REQUEST);
    let restore = is_true(&req, HX_RESTORE);
    let to_overlay_root = req
        .headers()
        .get(HX_TARGET)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|t| t == OVERLAY_ROOT);
    let here = req
        .uri()
        .path_and_query()
        .map(ToString::to_string)
        .unwrap_or_default();
    let is_get = req.method() == axum::http::Method::GET;
    // The page behind the overlay the script posted from (a same-origin path only): where a page answered to a POST belongs.
    let parent = req
        .headers()
        .get("x-overlay-parent")
        .and_then(|v| v.to_str().ok())
        .filter(|p| p.starts_with('/') && !p.starts_with("//") && !p.contains(['\r', '\n']))
        .map(ToOwned::to_owned);
    let preloading = is_true(&req, HX_PRELOADED) && req.method() == axum::http::Method::GET;
    let asked = InPlace::of(&req, hx && !restore);
    let canon = Arc::new(Mutex::new(None));
    let mut res = CANONICAL
        .scope(
            Arc::clone(&canon),
            REGION.scope(hx && !restore, next.run(req)),
        )
        .await;
    if res.status().is_success()
        && let (Some(asked), Some(at)) = (asked, canon.lock().ok().and_then(|g| g.clone()))
    {
        asked.put(res.headers_mut(), &at);
    }
    let h = res.headers_mut();
    h.append(
        axum::http::header::VARY,
        HeaderValue::from_static("HX-Request, HX-History-Restore-Request, HX-Target"),
    );
    // A page fetched ahead of a click (the pointer rests on a link) may be used by that click for a few seconds, from the
    // browser's cache; every other answer is stored by no one.
    if preloading && res.status().is_success() {
        res.headers_mut().insert(
            axum::http::header::CACHE_CONTROL,
            HeaderValue::from_static("private, max-age=5"),
        );
    }
    // A page where an overlay was asked for (a write's result): swap it into `main`, replace the overlay's history entry.
    if hx && !restore && to_overlay_root && res.status().is_success() && !is_overlay(&res) {
        let h = res.headers_mut();
        for (k, v) in [
            ("hx-retarget", "#main"),
            ("hx-reswap", "outerHTML transition:true show:window:top"),
        ] {
            h.insert(HeaderName::from_static(k), HeaderValue::from_static(v));
        }
        // The page's own address replaces the overlay's, when this answer is a page's own GET (the redirect after a write). A
        // POST answered with a page directly has none: the page behind the overlay it came from (or, told of none, the overlay's) stays.
        let at = if is_get {
            here
        } else {
            parent.unwrap_or_else(|| "false".to_owned())
        };
        if let Ok(v) = HeaderValue::from_str(&at) {
            h.insert(HeaderName::from_static("hx-replace-url"), v);
        }
    }
    res
}

/// What an in-place control (a request that targets a region of the page, not `main` or the overlay) says about the history.
struct InPlace {
    /// The address the page is at now (`HX-Current-URL`, path and query).
    current: String,
    /// Typing in a search box: replace the entry instead of adding one.
    replace: bool,
}

impl InPlace {
    fn of(req: &Request, region: bool) -> Option<Self> {
        let h = req.headers();
        let target = h.get(HX_TARGET).and_then(|v| v.to_str().ok()).unwrap_or("");
        if !region
            || req.method() != axum::http::Method::GET
            || target.is_empty()
            || target == MAIN
            || target == OVERLAY_ROOT
        {
            return None;
        }
        let current = h
            .get("hx-current-url")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let current = current.split_once("://").map_or(current, |(_, rest)| {
            rest.find('/').map_or("/", |i| &rest[i..])
        });
        let replace = h
            .get(crate::ui::inplace::HEADER_HISTORY)
            .and_then(|v| v.to_str().ok())
            == Some(crate::ui::inplace::HISTORY_REPLACE);
        Some(Self {
            current: current.split('#').next().unwrap_or("").to_owned(),
            replace,
        })
    }

    /// The history headers: the clean address replaces the one the form made; a control that changes nothing adds no entry.
    fn put(&self, h: &mut axum::http::HeaderMap, at: &str) {
        let (name, value) = if self.replace {
            ("hx-replace-url", at)
        } else if at == self.current {
            ("hx-push-url", "false")
        } else {
            ("hx-push-url", at)
        };
        if let Ok(v) = HeaderValue::from_str(value) {
            h.insert(HeaderName::from_static(name), v);
        }
    }
}

fn is_overlay(res: &Response) -> bool {
    res.headers()
        .contains_key(crate::ui::client::HEADER_OVERLAY)
}

#[cfg(test)]
mod tests;
