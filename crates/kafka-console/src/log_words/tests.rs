use super::*;
use crate::fixtures_log::log;

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
fn a_log_that_is_due_says_its_share_the_threshold_and_when_the_cleaner_looks() {
    assert_eq!(
        text(&head_sentence(&view("inventory.levels"))),
        "61,904 offsets in 8 segments, 39.7 MiB. 61 % of the cleanable log is dirty, more than the 50 % it waits for, so the cleaner will take it at its next look in 15 s."
    );
}

#[test]
fn a_log_that_is_not_due_says_why_it_waits() {
    assert_eq!(
        text(&head_sentence(&view("customer.profiles"))),
        "4,402 offsets in 3 segments, 9.8 MiB. 31 % of the log is dirty and the cleaner waits for more than 50 %, so it is looking at this log every 15 s and leaving it alone."
    );
}

#[test]
fn an_empty_log_says_how_a_partition_begins() {
    assert_eq!(
        text(&head_sentence(&view("orders.dlq"))),
        "No records yet. A new partition starts with one empty active segment at offset 0; the log grows from there."
    );
}

#[test]
fn a_log_everything_was_deleted_from_says_where_it_starts_now() {
    let mut v = view("orders.dlq");
    v.never_written = false;
    v.end = 150;
    v.start = 150;
    assert_eq!(
        text(&head_sentence(&v)),
        "No records left. Everything before offset 150 was deleted, by retention or by Delete records; the log grows from there."
    );
}

#[test]
fn the_cleaner_being_off_or_running_or_failing_changes_the_sentence_and_the_last_rule() {
    let mut v = view("inventory.levels");
    v.compaction.as_mut().unwrap().cleaner = LogCleaner::Disabled;
    assert!(text(&head_sentence(&v)).contains("switched off (--log-cleaner-enable)"));
    assert_eq!(rules(&v).last().unwrap().tone, RuleTone::No);
    v.compaction.as_mut().unwrap().cleaner = LogCleaner::Running { since_ms: 1 };
    assert!(text(&head_sentence(&v)).ends_with("A round is running on it now."));
    v.compaction.as_mut().unwrap().cleaner = LogCleaner::Failed {
        at_ms: 1,
        message: "disk full".into(),
    };
    let last = rules(&v).pop().unwrap();
    assert_eq!(
        (last.tone, last.title.as_str()),
        (RuleTone::No, "The last round failed")
    );
    assert_eq!(text(&last.sub), "disk full");
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
fn a_maximum_lag_forces_a_round_whatever_the_share() {
    let mut v = view("customer.profiles");
    let c = v.compaction.as_mut().unwrap();
    c.rules.max_lag_ms = Some(86_400_000);
    assert_eq!(rules(&v)[4].value, "1 d");
    let c = v.compaction.as_mut().unwrap();
    c.planner.waited_too_long = true;
    c.planner.due = true;
    let r = rules(&v);
    assert_eq!(
        (r[4].title.as_str(), r[4].value.as_str()),
        ("Forced by age", "overdue")
    );
    assert!(text(&head_sentence(&v)).contains("waited past max.compaction.lag.ms"));
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
fn retention_names_the_segment_that_turns_old_and_the_pass_that_takes_it() {
    let v = view("orders");
    assert_eq!(
        text(&head_sentence(&v)),
        "Segments older than 7 d are deleted whole, oldest first, every 5 m. Segment 1 turns 7 d old at 22:19 and goes at the pass at 22:22, which moves the log start from 0 to 5,980."
    );
    let e = expiry(&v).unwrap();
    assert_eq!(
        (e.going_now, e.new_start, e.next.map(|n| n.0)),
        (0, 5980, Some(0))
    );
    assert_eq!(
        e.pass_at_ms.map(crate::format::clock),
        Some("22:22:00".to_owned())
    );
}

#[test]
fn a_segment_already_past_retention_goes_at_the_next_pass() {
    let mut v = view("orders");
    v.retention.as_mut().unwrap().would_delete = 1;
    let e = expiry(&v).unwrap();
    assert_eq!((e.going_now, e.new_start), (1, 5980));
    assert!(
        text(&head_sentence(&v))
            .contains("Segment 1 is past it already and goes at the next pass (22:17)")
    );
}

#[test]
fn a_log_that_nothing_deletes_says_it_is_kept_for_ever_and_the_rules_say_no_limit() {
    let mut v = view("orders");
    let r = v.retention.as_mut().unwrap();
    (r.max_age_ms, r.max_bytes) = (None, None);
    assert!(text(&head_sentence(&v)).starts_with("Nothing deletes records here"));
    assert_eq!(expiry(&v), None);
    let rules = retention_rules(&v);
    assert_eq!(
        (rules[1].value.as_str(), rules[2].value.as_str()),
        ("none", "none")
    );
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
