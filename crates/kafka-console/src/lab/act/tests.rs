use kafka_server::fault::FaultTable;

use super::*;
use crate::lab::form::blank;

fn lab() -> Lab {
    Lab::new(FaultTable::new())
}

#[test]
fn a_rule_that_is_refused_by_the_engine_comes_back_in_its_form_with_the_reason() {
    let lab = lab();
    for n in 0..50 {
        let mut f = blank(LabKind::Slow);
        f.topic = format!("t{n}");
        assert!(matches!(
            make_rule(&lab, LabKind::Slow, f, Choices::default()),
            LabMade::Made(_)
        ));
    }
    let mut f = blank(LabKind::Slow);
    f.topic = "one-more".into();
    let LabMade::Invalid(v) = make_rule(&lab, LabKind::Slow, f, Choices::default()) else {
        panic!()
    };
    assert!(v.refused.unwrap().contains("50 rules"));
    assert!(v.preview.is_some());
}

#[test]
fn a_replaced_rule_keeps_the_count() {
    let lab = lab();
    for _ in 0..3 {
        let mut f = blank(LabKind::Fail);
        f.topic = "orders".into();
        let _ = make_rule(&lab, LabKind::Fail, f, Choices::default());
    }
    assert_eq!(lab.table().len(), 1);
}

#[test]
fn a_member_choice_round_trips_hostile_names() {
    let v = member_value("g/roup%", "m=1&x");
    assert_eq!(split_member(&v), Some(("g/roup%".into(), "m=1&x".into())));
    assert_eq!(split_member("nothing"), None);
}

#[test]
fn drop_once_needs_an_open_connection_and_a_plain_name() {
    let lab = lab();
    let f = drop_form(&[("catalog".into(), 2)], "catalog", "once");
    assert_eq!(f.call, "doze:DropConnection client=catalog once");
    assert!(matches!(
        drop_it(&lab, f.clone(), 0, Some(3)),
        LabOnceDone::Invalid(_)
    ));
    assert!(matches!(drop_it(&lab, f, 2, Some(3)), LabOnceDone::Done));
    let r = lab.table().rules();
    assert_eq!(r[0].spec.max_hits, Some(2));
    assert_eq!(r[0].spec.matcher.up_to_conn, Some(3));
    let glob = drop_form(&[], "cat*", "5");
    assert!(
        matches!(drop_it(&lab, glob, 0, None), LabOnceDone::Invalid(_)),
        "a pattern is not an app"
    );
    let keep = drop_form(&[("catalog".into(), 1)], "catalog", "5");
    assert!(matches!(drop_it(&lab, keep, 1, None), LabOnceDone::Done));
}

#[test]
fn a_history_of_rehearsals_is_bounded() {
    let lab = lab();
    for n in 0..(crate::lab::MOST_ONCE + 30) {
        lab.note(LabOnceKind::Clean, format!("{n}"), true);
    }
    assert_eq!(lab.once().len(), crate::lab::MOST_ONCE);
    assert_eq!(
        lab.once()[0].sentence,
        format!("{}", crate::lab::MOST_ONCE + 29)
    );
}
