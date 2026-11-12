use super::*;
use Mechanism::{Plain, ScramSha256, ScramSha512};

fn users() -> Credentials {
    Credentials::new([("alice".to_owned(), "alice-secret".to_owned())]).unwrap()
}

#[test]
fn the_buckets_are_the_ones_java_computes() {
    assert_eq!(bucket("PLAIN"), 0);
    assert_eq!(bucket("SCRAM-SHA-512"), 0);
    assert_eq!(bucket("SCRAM-SHA-256"), 6);
}

/// kafka-semantics.md 3.8: the table of orders, and what it says of the configured order.
#[test]
fn the_mechanism_list_is_in_java_hash_set_order() {
    let table: [(&[Mechanism], &[Mechanism]); 12] = [
        (&[Plain], &[Plain]),
        (&[ScramSha256], &[ScramSha256]),
        (&[ScramSha512], &[ScramSha512]),
        (&[Plain, ScramSha256], &[Plain, ScramSha256]),
        (&[ScramSha256, Plain], &[Plain, ScramSha256]),
        (&[Plain, ScramSha512], &[Plain, ScramSha512]),
        // Same bucket: the configured order stays.
        (&[ScramSha512, Plain], &[ScramSha512, Plain]),
        (&[ScramSha256, ScramSha512], &[ScramSha512, ScramSha256]),
        (&[ScramSha512, ScramSha256], &[ScramSha512, ScramSha256]),
        // Recorded: PLAIN, SCRAM-SHA-512, SCRAM-SHA-256.
        (
            &[Plain, ScramSha512, ScramSha256],
            &[Plain, ScramSha512, ScramSha256],
        ),
        (
            &[Plain, ScramSha256, ScramSha512],
            &[Plain, ScramSha512, ScramSha256],
        ),
        (
            &[ScramSha256, ScramSha512, Plain],
            &[ScramSha512, Plain, ScramSha256],
        ),
    ];
    for (configured, listed) in table {
        assert_eq!(java_hash_set_order(configured), listed, "{configured:?}");
    }
}

#[test]
fn a_mechanism_given_twice_is_listed_once() {
    assert_eq!(java_hash_set_order(&[Plain, Plain]), [Plain]);
}

#[test]
fn plain_alone_is_offered_and_logs_in() {
    let a = Authenticator::new(&[Plain], users()).unwrap();
    assert_eq!(a.offered(), [Plain]);
    assert!(a.offers(Plain) && !a.offers(ScramSha256));
    assert_eq!(
        a.plain(b"\0alice\0alice-secret").unwrap().as_str(),
        "User:alice"
    );
    assert_eq!(a.plain(b"\0alice\0nope"), Err(Refusal::BadCredentials));
}

#[test]
fn a_setup_that_cannot_work_is_refused() {
    assert_eq!(
        Authenticator::new(&[], users()).unwrap_err(),
        SetupError::NoMechanism
    );
    assert_eq!(
        Authenticator::new(&[Plain], Credentials::default()).unwrap_err(),
        SetupError::NoUsers
    );
    assert_eq!(
        Authenticator::new(&[ScramSha256], Credentials::default()).unwrap_err(),
        SetupError::NoUsers
    );
}

#[test]
fn scram_is_offered_and_logs_in_with_the_mechanism_named() {
    use crate::scram::client::Client;
    let a = Authenticator::new(&[Plain, ScramSha256, ScramSha512], users()).unwrap();
    assert_eq!(a.offered(), [Plain, ScramSha512, ScramSha256]);
    for (mechanism, kind) in [(ScramSha256, Kind::Sha256), (ScramSha512, Kind::Sha512)] {
        let c = Client::new(kind, "alice", "alice-secret", "nonce.");
        let (server_first, awaiting) = a.scram_first(mechanism, &c.first()).unwrap();
        let fin = c.last(&server_first, None, "");
        let (reply, who) = awaiting.finish(&fin.message).unwrap();
        assert_eq!((reply, who.as_str()), (fin.expected, "User:alice"));
    }
}

#[test]
fn scram_is_refused_for_a_mechanism_that_is_not_offered() {
    use crate::scram::client::Client;
    let a = Authenticator::new(&[Plain, ScramSha256], users()).unwrap();
    let c = Client::new(Kind::Sha512, "alice", "alice-secret", "nonce.");
    for mechanism in [ScramSha512, Plain] {
        let r = a.scram_first(mechanism, &c.first()).unwrap_err();
        assert!(matches!(r, scram::Refusal::Rejected { .. }) && !r.is_silent());
    }
}

#[test]
fn debug_output_shows_no_password() {
    let a = Authenticator::new(&[Plain], users()).unwrap();
    let shown = format!("{a:?} {a:#?}");
    assert!(
        shown.contains("alice") && !shown.contains("alice-secret"),
        "{shown}"
    );
}
