use super::*;
use crate::fixtures_log::log;
use crate::model::Span;

fn text(spans: &[Span]) -> String {
    spans
        .iter()
        .map(|s| match s {
            Span::Text(t) | Span::Mono(t) | Span::Strong(t) => t.as_str(),
        })
        .collect()
}

fn view(topic: &str) -> LogView {
    log(topic, 0).unwrap()
}

#[test]
fn each_rule_of_the_planner_carries_this_partitions_numbers() {
    let r = rules(&view("inventory.levels"));
    assert_eq!(r.len(), 7);
    assert!(r.iter().all(|x| x.tone == RuleTone::Ok));
    assert_eq!(r[1].value, "61 % > 50 %");
    assert_eq!(
        text(&r[1].sub),
        "24.0 MiB dirty of 39.1 MiB cleanable (clean 15.1 MiB); needs more than min.cleanable.dirty.ratio 0.5"
    );
    assert_eq!(
        (r[2].value.as_str(), r[3].value.as_str()),
        ("0 ms", "LSO 61,904")
    );
    assert_eq!(r[5].value, "up to 61,300");
    let mut dirty = rules(&view("customer.profiles"));
    let share = dirty.remove(1);
    assert_eq!(
        (share.tone, share.value.as_str()),
        (RuleTone::No, "31 % \u{2264} 50 %")
    );
    assert_eq!(share.title, "The dirty share is not over the threshold");
}

#[test]
fn an_open_transaction_holds_cleaning_back_and_a_minimum_lag_makes_a_segment_too_new() {
    let mut v = view("inventory.levels");
    v.last_stable = 61_000;
    let t = &rules(&v)[3];
    assert_eq!((t.tone, t.value.as_str()), (RuleTone::No, "LSO 61,000"));
    assert!(text(&t.sub).contains("904 below the high watermark 61,904"));
    let c = v.compaction.as_mut().unwrap();
    c.planner.limit = crate::model::LogLimit::TooNew;
    c.rules.min_lag_ms = 3_600_000;
    let t = &rules(&v)[2];
    assert_eq!((t.tone, t.value.as_str()), (RuleTone::No, "1 h"));
}

#[test]
fn the_next_round_names_the_segments_it_rewrites_and_where_the_checkpoint_goes() {
    assert_eq!(
        text(&would_rewrite(&view("inventory.levels"))),
        "A round will rewrite segments 5 to 7 (24.0 MiB) in place, keep the latest record of each key, and move the checkpoint to 61,300."
    );
    let mut v = view("inventory.levels");
    v.compaction.as_mut().unwrap().planner.upto = 48_877;
    assert!(text(&would_rewrite(&v)).starts_with("A round will rewrite segment 5 ("));
}

#[test]
fn what_would_make_a_log_due_is_the_threshold_in_bytes() {
    let v = view("customer.profiles");
    let (needs, has, _) = still_to_come(&v, v.compaction.as_ref().unwrap());
    // Clean is 6.1 MiB and the ratio 0.5: dirty must pass the clean part.
    assert_eq!((needs.as_str(), has.as_str()), ("6.1 MiB", "2.7 MiB"));
}

#[test]
fn a_size_limit_says_what_the_partition_holds() {
    let mut v = view("orders");
    v.retention.as_mut().unwrap().max_bytes = Some(1 << 20);
    let rules = retention_rules(&v);
    assert_eq!(rules[2].title, "Size: 1.0 MiB");
    assert_eq!(rules[2].value, "over");
}

#[test]
fn the_rules_of_retention_say_when_the_passes_are() {
    let r = retention_rules(&view("orders"));
    assert_eq!(r.len(), 5);
    assert_eq!(r[4].title, "Checked every 5 m");
    assert_eq!(r[4].value, "22:17");
    assert_eq!(r[1].value, "turns 7 d at 22:19");
}

#[test]
fn a_share_is_a_whole_per_cent_and_a_ratio_reads_as_the_config_does() {
    assert_eq!(
        (pct(0.614), pct(0.5), pct(0.0)),
        ("61 %".to_owned(), "50 %".to_owned(), "0 %".to_owned())
    );
    assert_eq!(
        (ratio_text(0.5), ratio_text(0.7), ratio_text(1.0)),
        ("0.5".to_owned(), "0.7".to_owned(), "1".to_owned())
    );
}

#[test]
fn a_share_exactly_at_the_threshold_is_not_over_it() {
    let mut v = view("customer.profiles");
    let c = v.compaction.as_mut().unwrap();
    (c.planner.dirty_bytes, c.planner.clean_bytes) = (100, 100);
    let r = rules(&v);
    assert_eq!(
        (r[1].tone, r[1].value.as_str()),
        (RuleTone::No, "50 % \u{2264} 50 %")
    );
    v.compaction.as_mut().unwrap().planner.dirty_bytes = 101;
    assert_eq!(rules(&v)[1].tone, RuleTone::Ok);
}
