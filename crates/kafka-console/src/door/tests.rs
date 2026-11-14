use std::net::IpAddr;

use super::*;

fn ip(text: &str) -> Option<IpAddr> {
    Some(text.parse().unwrap())
}

#[test]
fn a_watched_console_refuses_other_machines_exactly_while_the_broker_has_acls() {
    use Verdict::{Answer, Refuse};
    // (watched, peer, ACLs) -> what the door does.
    let table = [
        // Not watched (loopback console, or --console-unprotected): never refused.
        (false, ip("192.168.1.20"), 3, Answer),
        (false, None, 3, Answer),
        // Watched, no ACL: the broker is open, so is the console.
        (true, ip("192.168.1.20"), 0, Answer),
        (true, None, 0, Answer),
        // Watched with ACLs: another machine is refused, this one is not.
        (true, ip("192.168.1.20"), 1, Refuse),
        (true, ip("10.0.0.7"), 40, Refuse),
        (true, ip("fe80::1"), 1, Refuse),
        (true, ip("127.0.0.1"), 1, Answer),
        (true, ip("127.3.4.5"), 1, Answer),
        (true, ip("::1"), 1, Answer),
        (true, ip("::ffff:127.0.0.1"), 1, Answer),
        (true, ip("::ffff:192.168.1.20"), 1, Refuse),
        // A request whose peer is not known is not taken to be this machine.
        (true, None, 1, Refuse),
    ];
    for (watched, peer, acls, want) in table {
        assert_eq!(
            verdict(watched, peer, acls),
            want,
            "{watched} {peer:?} {acls}"
        );
    }
}

#[test]
fn the_refusal_says_what_happened_why_and_both_ways_forward() {
    let with = words(&Door {
        local: Some("http://127.0.0.1:9093/".to_owned()),
    });
    for part in [
        "This broker now has ACLs, so the console no longer answers other machines",
        "acts as a super user, so it would get round them",
        "use it on the machine doze-kafka runs on, at http://127.0.0.1:9093/",
        "restart doze-kafka with --console-unprotected to accept that",
    ] {
        assert!(with.contains(part), "{with}");
    }
    let without = words(&Door { local: None });
    assert!(
        without.contains("serve it on loopback (--console-addr 127.0.0.1:PORT)"),
        "{without}"
    );
    assert!(without.contains("--console-unprotected"), "{without}");
}

#[test]
fn the_log_says_once_when_the_door_closes_and_once_when_it_opens_again() {
    assert!(
        said(0, 1)
            .unwrap()
            .contains("no longer answers other machines")
    );
    assert!(said(0, 12).is_some());
    // More ACLs, or fewer but some: the door was closed and stays closed, nothing is said.
    assert_eq!(said(1, 2), None);
    assert_eq!(said(5, 1), None);
    assert_eq!(said(0, 0), None);
    assert!(said(3, 0).unwrap().contains("answers other machines again"));
}
