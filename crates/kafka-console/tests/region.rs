//! The region contract (`src/region.rs`, `docs/port/console/client-stack.md`): a request from htmx is answered with the page's
//! region (title, `main`, the rail and the overlay root as out-of-band parts), never the shell; any other request, and a
//! history restore, is answered with the whole document.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic)]

mod support;

use axum::body::Body;
use axum::http::{Request, header};
use support::{HOST, app, send};

fn hx(path: &str, headers: &[(&str, &str)]) -> Request<Body> {
    let mut b = Request::builder()
        .uri(path)
        .header(header::HOST, HOST)
        .header("hx-request", "true");
    for (k, v) in headers {
        b = b.header(*k, *v);
    }
    b.body(Body::empty()).unwrap()
}

#[tokio::test]
async fn an_htmx_request_gets_the_region_and_a_plain_one_the_whole_document() {
    let (app, _) = app("/", &[]);
    let (res, whole) = send(&app, support::get("/topics")).await;
    assert!(res.status().is_success());
    assert!(whole.starts_with("<!DOCTYPE html>") && whole.contains("<header class=\"topbar\">"));
    let (res, region) = send(&app, hx("/topics", &[("hx-target", "main")])).await;
    assert!(res.status().is_success());
    assert!(
        region.starts_with("<title>Topics · doze-kafka</title><main class=\"main\" id=\"main\""),
        "{}",
        &region[..120.min(region.len())]
    );
    for shell in ["<!DOCTYPE", "<header class=\"topbar\">", "<head>", "<script"] {
        assert!(!region.contains(shell), "the region carries {shell}");
    }
    assert!(region.contains(r#"<nav class="rail" id="rail" aria-label="Spaces" hx-swap-oob="true">"#));
    assert!(region.contains(r#"<div id="ovl" hx-swap-oob="true"></div>"#), "{region}");
    assert_eq!(region.matches("<main ").count(), 1);
    let vary = res.headers().get_all(header::VARY).iter().map(|v| v.to_str().unwrap()).collect::<Vec<_>>().join(",");
    assert!(vary.contains("HX-Request") && vary.contains("HX-Target"), "{vary}");
}

#[tokio::test]
async fn a_history_restore_wants_the_whole_document() {
    let (app, _) = app("/", &[]);
    let (_, doc) = send(&app, hx("/topics", &[("hx-history-restore-request", "true")])).await;
    assert!(doc.starts_with("<!DOCTYPE html>"), "{}", &doc[..60]);
}

#[tokio::test]
async fn an_overlay_opened_by_htmx_is_the_element_alone_and_a_boosted_visit_is_the_region_with_the_overlay_in_the_root() {
    let (app, _) = app("/", &[]);
    let (res, only) = send(&app, hx("/topics/orders/delete", &[("hx-target", "ovl")])).await;
    assert!(res.headers().contains_key("x-overlay"));
    assert!(only.starts_with("<div class=\"ovl ovl-dialog root\""), "{}", &only[..80]);
    let (res, region) = send(&app, hx("/topics/orders/delete", &[("hx-target", "main"), ("hx-boosted", "true")])).await;
    assert!(!res.headers().contains_key("x-overlay"));
    assert!(region.starts_with("<title>"), "{}", &region[..60]);
    assert!(region.contains(r#"<div id="ovl" hx-swap-oob="true"><div class="ovl ovl-dialog root""#), "{region}");
}

#[tokio::test]
async fn a_page_answered_to_a_request_for_the_overlay_root_says_where_it_goes() {
    let (app, _) = app("/", &[]);
    let (res, _) = send(&app, hx("/topics", &[("hx-target", "ovl")])).await;
    let h = res.headers();
    assert_eq!(h.get("hx-retarget").unwrap(), "#main");
    assert!(h.get("hx-reswap").unwrap().to_str().unwrap().contains("transition:true"));
    assert_eq!(h.get("hx-replace-url").unwrap(), "true");
    // An overlay answered to the overlay root is left alone.
    let (res, _) = send(&app, hx("/topics/orders/delete", &[("hx-target", "ovl")])).await;
    assert!(!res.headers().contains_key("hx-retarget"));
}

#[tokio::test]
async fn only_a_preloaded_page_may_be_kept_by_the_browser_and_only_for_seconds() {
    let (app, _) = app("/", &[]);
    let (res, _) = send(&app, hx("/topics", &[("hx-preloaded", "true")])).await;
    assert_eq!(res.headers().get(header::CACHE_CONTROL).unwrap(), "private, max-age=5");
    let (res, _) = send(&app, hx("/topics", &[])).await;
    assert_eq!(res.headers().get(header::CACHE_CONTROL).unwrap(), "no-store");
}

#[tokio::test]
async fn the_document_names_the_scripts_in_load_order_and_configures_htmx_without_eval() {
    let (app, s) = app("/", &[]);
    let (_, doc) = send(&app, support::get("/")).await;
    let at = |needle: &str| doc.find(needle).unwrap_or_else(|| panic!("{needle} not in the head"));
    let order = ["htmx-2.0.11", "idiomorph-ext", "htmx-preload", "console.", "components.", "alpine-anchor", "alpine-csp"];
    let positions: Vec<usize> = order.iter().map(|n| at(&format!("/static/{n}"))).collect();
    assert!(positions.windows(2).all(|w| w[0] < w[1]), "{positions:?}");
    assert!(doc.contains(r#"<meta name="htmx-config" content="{&quot;allowEval&quot;:false,&quot;allowScriptTags&quot;:false,&quot;selfRequestsOnly&quot;:true"#), "{doc}");
    assert!(doc.contains("<body hx-boost=\"true\""));
    assert_eq!(doc.matches("<script").count(), s.assets.urls.scripts.len());
    assert!(!doc.contains("<script>"), "an inline script");
}
