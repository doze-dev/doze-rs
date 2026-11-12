use super::*;

fn back(referer: &str) -> Option<String> {
    back_path(Some(referer), Some("127.0.0.1:9093"), &Base::root(), "/lab")
}

#[test]
fn the_way_back_is_the_page_we_came_from_when_it_is_one_of_ours() {
    assert_eq!(
        back("http://127.0.0.1:9093/topics/orders/messages?partition=0").as_deref(),
        Some("/topics/orders/messages?partition=0")
    );
    assert_eq!(back("http://127.0.0.1:9093/").as_deref(), Some("/"));
    assert_eq!(
        back_path(
            Some("https://Example.test/groups"),
            Some("example.test"),
            &Base::root(),
            "/lab"
        )
        .as_deref(),
        Some("/groups")
    );
}

#[test]
fn a_referer_that_is_not_ours_or_not_a_page_is_no_way_back() {
    assert_eq!(back("http://evil.test/topics"), None);
    assert_eq!(back("http://127.0.0.1:9094/topics"), None);
    assert_eq!(back("http://127.0.0.1:9093"), None);
    assert_eq!(back("http://127.0.0.1:9093//evil.test/x"), None);
    assert_eq!(back("javascript:alert(1)"), None);
    assert_eq!(
        back("http://127.0.0.1:9093/lab"),
        None,
        "the page itself is no way back"
    );
    assert_eq!(back("http://127.0.0.1:9093/lab?x=1"), None);
    assert_eq!(back("http://127.0.0.1:9093/a\"onmouseover=\"x"), None);
    assert_eq!(back_path(None, Some("h"), &Base::root(), "/lab"), None);
    assert_eq!(
        back_path(Some("http://h/x"), None, &Base::root(), "/lab"),
        None
    );
}

#[test]
fn under_a_prefix_only_a_page_of_the_console_counts() {
    let base = Base::new("/k");
    let back = |r: &str| back_path(Some(r), Some("h"), &base, "/k/lab");
    assert_eq!(back("http://h/k/topics").as_deref(), Some("/k/topics"));
    assert_eq!(back("http://h/other"), None);
}

#[test]
fn each_unbuilt_screen_knows_its_section() {
    let (p, rail, section, _) = place("Producers", "orders", "");
    assert_eq!(p.steps().last().map(|s| s.0.as_str()), Some("Producers"));
    assert_eq!(
        (rail, section.as_str()),
        (Rail::Resources, "/topics/orders")
    );
    let (p, rail, section, _) = place("Lab", "", "");
    assert_eq!(
        (p.steps().len(), rail, section.as_str()),
        (1, Rail::Lab, "/")
    );
    let (p, _, section, _) = place("Story", "", "a b");
    assert_eq!(p.steps()[3].0, "Story");
    assert_eq!(section, "/groups/a%20b");
}
