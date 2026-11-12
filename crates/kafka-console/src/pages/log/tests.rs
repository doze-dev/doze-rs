use super::*;

fn head(topic: &str) -> TopicHead {
    crate::fixtures_topics::head(topic).unwrap()
}
use crate::fixtures_log::log;

fn page(topic: &str) -> String {
    log_page(
        &log(topic, 0).unwrap(),
        &head(topic),
        &Base::root(),
        "127.0.0.1:9092",
    )
    .into_string()
}

#[test]
fn the_four_boards_are_the_four_states_of_one_page_chosen_by_the_data() {
    assert!(page("inventory.levels").contains("Will the cleaner take it?"));
    assert!(page("customer.profiles").contains("Why it waits"));
    assert!(page("orders").contains("What the next pass will do"));
    assert!(page("orders.dlq").contains("This log has nothing in it"));
}

#[test]
fn an_empty_log_has_no_segments_table_no_cards_and_no_other_logs() {
    let p = page("orders.dlq");
    for absent in [
        "Segments</h2>",
        "Durability",
        "Other logs on this broker",
        "planner",
    ] {
        assert!(!p.contains(absent), "{absent}");
    }
    assert!(p.contains("kcat -P -b 127.0.0.1:9092 -t orders.dlq"));
}

#[test]
fn no_page_of_the_log_has_a_form_a_write_or_a_compact_now_button() {
    for t in [
        "inventory.levels",
        "customer.profiles",
        "orders",
        "orders.dlq",
    ] {
        let p = page(t);
        assert!(
            !p.contains("<form") && !p.contains("method=\"post\""),
            "{t}"
        );
        assert!(!p.contains("Compact now"), "{t}");
    }
}

#[test]
fn the_live_parts_are_fragments_the_stream_can_name() {
    let p = page("inventory.levels");
    for id in [LOG_SENTENCE, LOG_RULER, LOG_CARDS] {
        assert!(p.contains(&format!("id=\"{id}\"")), "{id}");
        assert!(
            p.contains(&format!(
                "data-frag=\"/fragments/{id}?topic=inventory.levels&amp;partition=0\""
            )),
            "{id}"
        );
    }
}

#[test]
fn a_topic_name_that_is_hostile_is_escaped_everywhere_it_is_shown() {
    let mut v = log("inventory.levels", 0).unwrap();
    v.topic = "<script>alert(1)</script>\"'".to_owned();
    v.others[0].topic = "<img src=x onerror=1>".to_owned();
    v.tombstones[0].key = "<b>key</b>".to_owned();
    let mut h = head("inventory.levels");
    h.name = v.topic.clone();
    let p = log_page(&v, &h, &Base::root(), "127.0.0.1:9092").into_string();
    for raw in ["<script>alert", "<img src=x", "<b>key</b>"] {
        assert!(!p.contains(raw), "{raw}");
    }
    assert!(p.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
}

#[test]
fn a_topic_with_several_partitions_says_which_it_shows_and_links_to_the_others() {
    let p = page("orders");
    assert!(p.contains("partition 0 of 3"));
    assert!(p.contains("href=\"/topics/orders/log?partition=2\""));
}

#[test]
fn the_segments_table_opens_the_segment_that_holds_the_checkpoint() {
    let p = page("inventory.levels");
    assert_eq!(p.matches("<details").count(), 8);
    assert_eq!(p.matches("<details open").count(), 1);
    assert!(p.contains("holds the checkpoint"));
    assert!(p.contains("00000000000000039195.log"));
}
