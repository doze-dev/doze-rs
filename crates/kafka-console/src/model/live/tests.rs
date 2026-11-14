use super::*;

fn q(s: &str) -> WireQuery {
    WireQuery::parse(&crate::form::parse(s))
}

#[test]
fn an_address_is_the_filters_and_back_again() {
    let w =
        q("q=ord&client=admin-cli&api=Produce&topic=orders&refused=1&idle=0&before=12&limit=50");
    assert_eq!(w.text, "ord");
    assert_eq!(
        (w.client.clone(), w.api.clone(), w.topic.clone()),
        (
            vec!["admin-cli".to_owned()],
            vec!["Produce".to_owned()],
            vec!["orders".to_owned()]
        )
    );
    assert!(w.refused_only && !w.hide_idle);
    assert_eq!((w.before, w.limit), (Some(12), 50));
    assert_eq!(q(&w.to_query()[1..]), w);
    assert_eq!(WireQuery::default().to_query(), "");
}

#[test]
fn nonsense_is_no_filter_and_the_limit_is_held_to_its_range() {
    assert_eq!(q("before=abc&until=-1&limit=999999").limit, WIRE_MOST);
    assert_eq!(q("limit=0").limit, 1);
    assert_eq!(q("before=abc").before, None);
    assert!(q("idle=yes").hide_idle);
    assert_eq!(q(&format!("q={}", "x".repeat(1000))).text.len(), 128);
}

#[test]
fn a_click_on_a_cell_filters_by_it_and_leaves_the_page_back() {
    let w = q("q=a&before=9").with("client", "orders");
    assert_eq!(
        (w.client.clone(), w.before, w.text.as_str()),
        (vec!["orders".to_owned()], None, "a")
    );
    assert_eq!(w.with("nothing", "x"), w);
    assert!(w.with("client", "").client.is_empty());
}

#[test]
fn a_repeated_field_is_several_values_in_the_address_and_back() {
    let w = q(
        "client=a&client=b&client=a&topic=orders&topic=%20payments.settled%20&topic=&api=produce&api=Fetch&api=nope",
    );
    assert_eq!(w.client, ["a", "b"], "no repeats");
    assert_eq!(
        w.topic,
        ["orders", "payments.settled"],
        "trimmed, no empties"
    );
    assert_eq!(
        w.api,
        ["Produce", "Fetch"],
        "the broker's names; what is no API is dropped"
    );
    assert_eq!(
        w.to_query(),
        "?client=a&client=b&api=Produce&api=Fetch&topic=orders&topic=payments.settled"
    );
    assert_eq!(q(&w.to_query()[1..]), w);
    assert_eq!(w.without("client", "a").client, ["b"]);
    assert!(w.is_filtered() && !WireQuery::default().is_filtered());
}

#[test]
fn the_values_of_a_kind_are_bounded() {
    let many: String = (0..100).map(|i| format!("topic=t{i}&")).collect();
    // The form reader keeps 64 fields; a kind keeps at most MOST_VALUES of them.
    assert_eq!(q(&many).topic.len(), MOST_VALUES);
    assert_eq!(q(&format!("topic={}", "x".repeat(500))).topic[0].len(), 128);
}

#[test]
fn a_kind_matches_any_of_its_values_and_a_topic_matches_exactly() {
    let w = q("client=a&client=b&topic=orders");
    assert!(w.client_ok("a") && w.client_ok("b") && !w.client_ok("ab") && !w.client_ok("A"));
    assert!(w.topic_ok(["x", "orders"].into_iter()));
    assert!(
        !w.topic_ok(["orders.dlq", "backorders"].into_iter()),
        "not a substring"
    );
    assert!(!w.topic_ok(std::iter::empty()));
    assert!(q("").topic_ok(std::iter::empty()) && q("").client_ok("any") && q("").api_ok("Fetch"));
    assert!(q("api=fetch").api_ok("Fetch"));
}
