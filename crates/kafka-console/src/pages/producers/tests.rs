use super::*;
use crate::fixtures_log::{producers, txn_view};

fn page() -> String {
    producers_page(&producers("orders", 0, Vec::new()).unwrap(), &Base::root()).into_string()
}

#[test]
fn the_producer_with_an_open_transaction_shows_its_five_batches_and_the_others_a_link_to_open_theirs()
 {
    let p = page();
    assert!(p.contains("Producer 4017, the last 5 batches orders-0 remembers"));
    assert!(p.contains("22,915 to 22,918"));
    assert!(p.contains("Hide batches"));
    assert_eq!(p.matches("btn sm ghost").count(), 3);
    assert_eq!(p.matches("aria-expanded=\"false\"").count(), 2);
    assert!(!p.contains("Producer 4023, the last"));
    let opened =
        producers_page(&producers("orders", 0, vec![4023]).unwrap(), &Base::root()).into_string();
    assert!(opened.contains("Producer 4023, the last"));
    assert!(!opened.contains("Producer 4017, the last"));
}

#[test]
fn the_toggle_links_keep_the_other_open_rows_and_work_without_a_script() {
    let p = page();
    assert!(
        p.contains("href=\"/topics/orders/producers?open=4017,4023\""),
        "{p:.0}"
    );
    assert!(
        p.contains("href=\"/topics/orders/producers?open=\""),
        "hiding the only open row leaves none"
    );
}

#[test]
fn the_tail_marks_the_stable_offset_and_counts_the_held_records() {
    let p = page();
    assert!(p.contains("in the open transaction: 6 records a read_committed consumer cannot see"));
    assert!(p.contains("<b>16,098</b>") && p.contains("<b>16,104</b>"));
    assert_eq!(p.matches("class=\"sg active\"").count(), 6);
    assert_eq!(p.matches("class=\"sg sealed\"").count(), 3);
}

#[test]
fn the_tail_mark_sits_at_the_left_edge_of_its_cell() {
    assert_eq!(
        tail::mark_style(13, 19),
        "--at:calc((100% - 54px) * 0.68421 + 37.5px);"
    );
    assert_eq!(
        tail::mark_style(19, 19),
        "--at:calc((100% - 54px) * 1.00000 + 54px);transform:translateX(-100%);align-items:flex-end"
    );
}

#[test]
fn a_producer_is_shown_with_a_name_only_when_the_recorder_gave_one() {
    let mut v = producers("orders", 0, Vec::new()).unwrap();
    let p = producers_page(&v, &Base::root()).into_string();
    assert!(p.contains("orders (go, sarama)") && p.contains("admin-cli (kcat)"));
    assert_eq!(
        p.matches("not known").count(),
        1,
        "the third was never seen"
    );
    for row in &mut v.producers {
        row.seen_as.clear();
    }
    let p = producers_page(&v, &Base::root()).into_string();
    assert_eq!(p.matches("not known").count(), 3);
    assert!(!p.contains("orders (go, sarama)"));
    // A hostile client id is text.
    v.producers[0].seen_as = vec!["<img src=x> (go)".to_owned()];
    let p = producers_page(&v, &Base::root()).into_string();
    assert!(!p.contains("<img src=x>") && p.contains("&lt;img src=x&gt; (go)"));
}

#[test]
fn the_transaction_page_has_the_countdown_and_the_two_ways_on() {
    let v = txn_view("orders", "orders-eos").unwrap();
    let p = txn_page(&v, &Base::root()).into_string();
    assert!(p.contains("41 s of 60 s"));
    assert!(p.contains("19 s left, then the broker aborts it and fences the producer"));
    assert!(p.contains("href=\"/topics/orders/transactions/orders-eos/held\""));
    assert!(p.contains("Offsets staged") && p.contains("not listed"));
    let held = txn_held_page(&v, &Base::root()).into_string();
    assert!(
        held.contains("<dd class=\"mono\">16,103</dd>")
            && held.contains("<dd class=\"mono\">16,097</dd>")
    );
    assert!(held.contains("ListOffsets latest, read_committed"));
}

#[test]
fn hostile_ids_are_escaped_in_every_place_they_appear() {
    let mut v = producers("orders", 0, Vec::new()).unwrap();
    v.open[0].id = "<script>x</script>".to_owned();
    v.producers[0].txn.as_mut().unwrap().id = "<script>x</script>".to_owned();
    v.topic = "<i>t</i>".to_owned();
    let p = producers_page(&v, &Base::root()).into_string();
    assert!(!p.contains("<script>x") && !p.contains("<i>t</i>"));
    assert!(
        p.contains("%3Cscript%3Ex%3C/script%3E") || p.contains("%3Cscript%3Ex%3C%2Fscript%3E"),
        "{p:.0}"
    );
}
