//! What the ring keeps of a call a Lab rule touched.

use std::time::Duration;

use aws_gateway::Finished;

use super::tests::call;
use super::*;

#[test]
fn a_call_a_rule_touched_keeps_the_rule_and_is_found_by_it() {
    use aws_gateway::{Done, LabNote};
    let r = Recorder::holding(10);
    let touched = |rule, done| Finished {
        lab: Some(LabNote {
            rule,
            name: "a name the ring does not keep",
            done,
        }),
        ..call(1)
    };
    r.record(&call(1), 1);
    r.record(&touched(1, Done::Failed), 2);
    r.record(&touched(2, Done::Delayed(Duration::from_millis(250))), 3);
    r.record(&call(2), 4);
    r.record(&touched(1, Done::Dropped), 5);
    let lab = |seq| r.get(seq).unwrap().extra.and_then(|e| e.lab);
    assert_eq!(lab(1), None);
    assert_eq!(
        lab(2).map(|l| (l.rule(), l.did())),
        Some((1, LabDid::Failed))
    );
    assert_eq!(
        lab(3).map(|l| (l.rule(), l.did())),
        Some((2, LabDid::Delayed(250)))
    );
    assert_eq!(lab(5).map(|l| l.did()), Some(LabDid::Dropped));
    // Found by rule, newest first, and by any rule; a limit says whether there were more.
    let seqs = |rule, limit| {
        let (calls, more) = r.touched(rule, limit);
        (calls.iter().map(|c| c.seq).collect::<Vec<_>>(), more)
    };
    assert_eq!(seqs(Some(1), 10), (vec![5, 2], false));
    assert_eq!(seqs(Some(2), 10), (vec![3], false));
    assert_eq!(seqs(Some(9), 10), (vec![], false));
    assert_eq!(seqs(None, 10), (vec![5, 3, 2], false));
    assert_eq!(seqs(None, 2), (vec![5, 3], true));
    // A call that no rule touched holds no more than it did: the tag is part of the slot that holds the words of an error.
    assert_eq!(r.get(1).unwrap().bytes(), RECORD_BYTES + 36 + 4 + 9);
}

#[test]
fn a_lab_mark_is_eight_bytes_and_says_what_it_was_given() {
    assert_eq!(std::mem::size_of::<Option<Lab>>(), 8);
    assert!(
        Lab::new(0, LabDid::Failed).is_none(),
        "no table makes rule 0"
    );
    for did in [
        LabDid::Delayed(0),
        LabDid::Delayed(1),
        LabDid::Delayed(120_000),
        LabDid::Failed,
        LabDid::Dropped,
    ] {
        let mark = Lab::new(7, did).unwrap();
        assert_eq!((mark.rule(), mark.did()), (7, did));
    }
    // A wait too long to be one never reads as an error or a closed connection.
    let long = Lab::new(1, LabDid::Delayed(u32::MAX)).unwrap();
    assert!(matches!(long.did(), LabDid::Delayed(_)));
}
