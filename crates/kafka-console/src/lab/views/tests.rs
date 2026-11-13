use std::sync::Arc;

use kafka_server::fault::FaultTable;

use super::*;
use crate::lab::act::{Choices, make_rule};
use crate::lab::form::blank;
use crate::model::lab::{LabKind, LabMade};

#[test]
fn a_rule_in_force_and_one_over_are_the_same_view_with_and_without_an_end() {
    let lab = Lab::new(FaultTable::new());
    let mut f = blank(LabKind::Slow);
    f.topic = "orders".into();
    let LabMade::Made(id) = make_rule(&lab, LabKind::Slow, f, Choices::default()) else {
        panic!()
    };
    let h = home(&lab);
    assert_eq!(h.active.len(), 1);
    assert_eq!((h.active[0].id, h.active[0].ended), (id, None));
    assert!(h.most_rules == 50);
    assert!(lab.table().remove(id));
    let h = home(&lab);
    assert!(h.active.is_empty() && h.ended.len() == 1);
    assert_eq!(h.ended[0].ended, Some(LabEnd::Cleared));
    assert_eq!(find(&lab, id).unwrap().ended, Some(LabEnd::Cleared));
    assert!(find(&lab, 999).is_none());
    lab.clear_history();
    let h = home(&lab);
    assert!(h.active.is_empty() && h.ended.is_empty() && h.once.is_empty());
    let _ = Arc::new(0);
}
