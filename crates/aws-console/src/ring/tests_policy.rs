//! What the ring keeps of the policy that allowed a call.

use aws_gateway::{AllowedBy, Finished};

use super::tests::call;
use super::*;

#[test]
fn the_policy_that_allowed_a_call_is_kept_with_its_kind_and_cut_at_iams_limit() {
    let r = Recorder::holding(10);
    let by = |policy, kind| Finished {
        allowed_by: Some(AllowedBy { policy, kind }),
        ..call(1)
    };
    let long = "p".repeat(300);
    r.record(&call(1), 1);
    r.record(&by("PowerUserAccess", "aws-managed"), 2);
    r.record(&by(&long, "group"), 3);
    r.record(&by("x", "not-a-kind"), 4);
    let held = |seq| {
        r.get(seq)
            .unwrap()
            .extra
            .map(|e| (e.allowed_by.to_string(), e.allowed_kind))
    };
    assert_eq!(
        held(1),
        None,
        "a call no policy allowed holds nothing of it"
    );
    assert_eq!(
        held(2),
        Some(("PowerUserAccess".into(), Some(PolicyKind::AwsManaged)))
    );
    assert_eq!(
        held(3).map(|(n, k)| (n.len(), k)),
        Some((MOST_POLICY, Some(PolicyKind::Group)))
    );
    // A kind IAM does not have is not kept as one.
    assert_eq!(held(4), Some(("x".into(), None)));
    // A call with no policy holds no more than it did; one with a policy holds its name and a slot for the rest.
    assert_eq!(r.get(1).unwrap().bytes(), RECORD_BYTES + 36 + 4 + 9);
    assert_eq!(
        r.get(2).unwrap().bytes(),
        RECORD_BYTES + 36 + 4 + 9 + std::mem::size_of::<Extra>() + "PowerUserAccess".len()
    );
    assert_eq!(std::mem::size_of::<Option<PolicyKind>>(), 1);
}
