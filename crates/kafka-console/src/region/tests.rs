use super::*;

#[tokio::test]
async fn a_request_outside_the_layer_wants_the_whole_document() {
    assert!(!wanted());
}

#[tokio::test]
async fn scoped_says_what_the_request_in_flight_wants() {
    assert!(scoped(true, async { wanted() }).await);
    assert!(!scoped(false, async { wanted() }).await);
}

fn asked(replace: bool, current: &str) -> InPlace {
    InPlace {
        current: current.to_owned(),
        replace,
    }
}

fn header(h: &axum::http::HeaderMap, name: &str) -> Option<String> {
    h.get(name)
        .and_then(|v| v.to_str().ok())
        .map(ToOwned::to_owned)
}

#[test]
fn a_control_that_changes_the_address_pushes_the_clean_one() {
    let mut h = axum::http::HeaderMap::new();
    asked(false, "/topics?q=a").put(&mut h, "/topics?q=ab");
    assert_eq!(header(&h, "hx-push-url").as_deref(), Some("/topics?q=ab"));
    assert!(header(&h, "hx-replace-url").is_none());
}

#[test]
fn a_control_that_changes_nothing_adds_no_history_entry() {
    let mut h = axum::http::HeaderMap::new();
    asked(false, "/groups?state=stable").put(&mut h, "/groups?state=stable");
    assert_eq!(header(&h, "hx-push-url").as_deref(), Some("false"));
}

#[test]
fn typing_replaces_the_entry_with_the_clean_address() {
    let mut h = axum::http::HeaderMap::new();
    asked(true, "/topics?q=a").put(&mut h, "/topics");
    assert_eq!(header(&h, "hx-replace-url").as_deref(), Some("/topics"));
    assert!(header(&h, "hx-push-url").is_none());
}

#[test]
fn the_handler_names_the_clean_address_and_nothing_outside_a_request_does() {
    // Outside the layer there is nobody to tell: it is a no-op, not a panic.
    canonical("/x");
    let seen = Arc::new(Mutex::new(None));
    let got = tokio_test_block(async {
        CANONICAL
            .scope(Arc::clone(&seen), async {
                canonical("/topics?q=a");
            })
            .await;
        seen.lock().unwrap().clone()
    });
    assert_eq!(got.as_deref(), Some("/topics?q=a"));
}

fn tokio_test_block<F: std::future::Future>(f: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(f)
}
