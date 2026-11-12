use super::*;

fn facts() -> Facts {
    Facts {
        listen: "127.0.0.1:9092".parse().unwrap(),
        advertised_host: "127.0.0.1".into(),
        advertised_port: 9092,
        tls: false,
        sasl: vec![],
        kafka_major: 4,
        allow_hosts: vec![],
        unprotected: false,
    }
}

#[test]
fn a_specific_bind_is_the_bootstrap_and_a_wildcard_is_the_advertised_name() {
    let mut f = facts();
    assert_eq!(f.bootstrap(), "127.0.0.1:9092");
    assert_eq!(f.dial_listener(), "127.0.0.1:9092");
    assert!(f.loopback());
    f.listen = "0.0.0.0:9092".parse().unwrap();
    f.advertised_host = "localhost".into();
    assert_eq!(f.bootstrap(), "localhost:9092");
    assert_eq!(f.dial_listener(), "127.0.0.1:9092");
    assert!(!f.loopback());
    f.listen = "[::]:19092".parse().unwrap();
    assert_eq!(f.dial_listener(), "[::1]:19092");
    f.listen = "[::1]:9092".parse().unwrap();
    assert_eq!(f.bootstrap(), "[::1]:9092");
}

#[test]
fn an_advertised_ipv6_literal_is_bracketed_once() {
    let mut f = facts();
    f.advertised_host = "::1".into();
    assert_eq!(f.advertised(), "[::1]:9092");
    f.advertised_host = "[::1]".into();
    assert_eq!(f.advertised(), "[::1]:9092");
    f.advertised_host = "broker.internal".into();
    f.advertised_port = 19092;
    assert_eq!(f.advertised(), "broker.internal:19092");
}

#[test]
fn the_chip_says_what_is_on_in_the_boards_vocabulary() {
    let mut f = facts();
    let label = |f: &Facts, acls, allow| access(f, acls, allow).label;
    assert_eq!(label(&f, 0, true), "Open");
    assert_eq!(label(&f, 3, true), "Open · ACLs");
    assert_eq!(label(&f, 0, false), "Open · ACLs");
    f.sasl = vec!["SCRAM-SHA-256".into(), "SCRAM-SHA-512".into()];
    assert_eq!(label(&f, 0, true), "SASL SCRAM");
    f.sasl = vec!["PLAIN".into()];
    assert_eq!(label(&f, 0, true), "SASL PLAIN");
    f.sasl = vec!["PLAIN".into(), "SCRAM-SHA-256".into()];
    assert_eq!(label(&f, 8, false), "SASL · ACLs");
    f.sasl = vec!["SCRAM-SHA-256".into()];
    f.tls = true;
    assert_eq!(label(&f, 8, false), "SASL SCRAM · ACLs + TLS");
    f.sasl.clear();
    assert_eq!(label(&f, 0, true), "Open + TLS");
}

#[test]
fn the_access_row_of_home_reads_as_the_board_does_when_open() {
    let a = access(&facts(), 0, true);
    assert_eq!(a.detail, "No sign-in, no ACLs, plain TCP");
    assert!(!a.sasl);
    let mut f = facts();
    f.sasl = vec!["PLAIN".into()];
    f.tls = true;
    let a = access(&f, 1, false);
    assert_eq!(a.detail, "Sign-in required, 1 ACL, TLS");
    assert!(a.sasl);
    assert_eq!(
        access(&facts(), 0, false).detail,
        "No sign-in, ACLs deny by default, plain TCP"
    );
}

#[test]
fn the_chrome_locks_the_chip_when_anything_protects_the_broker() {
    let mut f = facts();
    assert!(!chrome(&f, &access(&f, 0, true)).locked);
    assert_eq!(chrome(&f, &access(&f, 0, true)).profile, "Kafka 4 profile");
    f.tls = true;
    assert!(chrome(&f, &access(&f, 0, true)).locked);
}
