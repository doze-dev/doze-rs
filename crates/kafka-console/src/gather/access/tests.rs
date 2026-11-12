use super::*;

fn scram(names: &[(&str, &[&'static str])]) -> Vec<(String, Vec<&'static str>)> {
    names
        .iter()
        .map(|(n, m)| ((*n).to_owned(), m.to_vec()))
        .collect()
}

#[test]
fn a_flag_user_is_from_flags_and_the_rest_are_stored_and_a_super_user_is_said() {
    let users = users_of(
        scram(&[
            ("payments", &["SCRAM-SHA-256"]),
            ("admin", &["SCRAM-SHA-256", "SCRAM-SHA-512"]),
            ("orders", &["SCRAM-SHA-512"]),
        ]),
        &["admin".to_owned()],
        false,
        &["User:admin".to_owned()],
    );
    let got: Vec<(&str, AccessFrom, bool)> = users
        .iter()
        .map(|u| (u.name.as_str(), u.from, u.super_user))
        .collect();
    assert_eq!(
        got,
        [
            ("admin", AccessFrom::Flags, true),
            ("orders", AccessFrom::Stored, false),
            ("payments", AccessFrom::Stored, false)
        ]
    );
    assert_eq!(users[0].mechanisms, ["SCRAM-SHA-256", "SCRAM-SHA-512"]);
}

#[test]
fn plain_is_a_mechanism_of_a_flag_user_only_when_it_is_on() {
    let off = users_of(
        scram(&[("a", &["SCRAM-SHA-256"])]),
        &["a".to_owned(), "b".to_owned()],
        false,
        &[],
    );
    assert_eq!(off[0].mechanisms, ["SCRAM-SHA-256"]);
    assert!(off[1].mechanisms.is_empty(), "{off:?}");
    let on = users_of(
        scram(&[("a", &["SCRAM-SHA-256"])]),
        &["a".to_owned(), "b".to_owned()],
        true,
        &[],
    );
    assert_eq!(on[0].mechanisms, ["SCRAM-SHA-256", "PLAIN"]);
    assert_eq!(on[1].mechanisms, ["PLAIN"]);
    assert!(on.iter().all(|u| u.from == AccessFrom::Flags));
}

#[test]
fn only_two_mechanism_numbers_have_names_and_the_iteration_count_is_not_one_of_the_fields_read() {
    assert_eq!(mechanism(1), Some("SCRAM-SHA-256"));
    assert_eq!(mechanism(2), Some("SCRAM-SHA-512"));
    assert_eq!(mechanism(0), None);
    assert_eq!(mechanism(3), None);
    // The user model has a name, an origin, mechanisms and a super-user mark: nothing else can be shown.
    let u = AccessUser {
        name: "n".into(),
        from: AccessFrom::Stored,
        mechanisms: vec![],
        super_user: false,
    };
    assert_eq!(
        format!("{u:?}"),
        "AccessUser { name: \"n\", from: Stored, mechanisms: [], super_user: false }"
    );
}

fn facts(sasl: &[&str], tls: bool) -> Facts {
    Facts {
        listen: "127.0.0.1:9092".parse().unwrap(),
        advertised_host: "127.0.0.1".into(),
        advertised_port: 9092,
        tls,
        sasl: sasl.iter().map(|s| (*s).to_owned()).collect(),
        kafka_major: 4,
        allow_hosts: vec!["kafka.test".into()],
        unprotected: false,
    }
}

fn entry(who: &str, perm: AclPermission) -> AclEntry {
    AclEntry {
        kind: 2,
        name: "orders".into(),
        pattern: AclPattern::Literal,
        principal: who.into(),
        host: "*".into(),
        operation: 4,
        permission: perm,
    }
}

#[test]
fn the_page_is_assembled_from_what_was_read_with_the_counts_the_board_writes() {
    let read = Read {
        flag_users: vec!["admin".into()],
        supers: vec!["User:admin".into()],
        default_allow: false,
        stored: 3,
        acls: Ok(vec![
            entry("User:a", AclPermission::Allow),
            entry("User:b", AclPermission::Allow),
            entry("User:c", AclPermission::Deny),
        ]),
        scram: scram(&[("admin", &["SCRAM-SHA-256"])]),
    };
    let start = StartFacts {
        tls_cert: Some("/c.pem".into()),
        failed_login_delay: Some(std::time::Duration::from_millis(100)),
        console_addr: Some("127.0.0.1:9093".into()),
        ..StartFacts::default()
    };
    let v = assemble(read, &facts(&["SCRAM-SHA-256"], true), &start);
    assert_eq!((v.authz.acls, v.authz.allow, v.authz.deny), (3, 2, 1));
    assert_eq!(
        (
            v.auth.flag_users,
            v.auth.stored_users,
            v.auth.on,
            v.auth.plain
        ),
        (1, 3, true, false)
    );
    assert_eq!(v.auth.failed_login.as_deref(), Some("waits 100 ms"));
    assert_eq!(v.tls.cert.as_deref(), Some("/c.pem"));
    assert_eq!(
        (v.console.listens.as_str(), v.console.allow_hosts.clone()),
        ("127.0.0.1:9093", vec!["kafka.test".to_owned()])
    );
    assert_eq!(v.users.len(), 1);
    assert!(
        v.warnings.iter().all(|w| !w.holds),
        "SCRAM, TLS, deny and loopback: no warning holds"
    );
}

#[test]
fn a_refused_describe_is_carried_as_the_refusal_and_the_counts_are_zero() {
    let read = Read {
        flag_users: vec![],
        supers: vec![],
        default_allow: true,
        stored: 0,
        acls: Err(crate::refusal::refusal(31, None)),
        scram: vec![],
    };
    let v = assemble(read, &facts(&[], false), &StartFacts::default());
    assert_eq!(v.acls.refused.as_ref().map(|r| r.code), Some(31));
    assert!(v.acls.rows.is_empty() && v.authz.acls == 0);
    assert_eq!(v.console.listens, "unknown", "no invented address");
}

#[test]
fn a_table_past_its_limit_says_how_many_it_left_out() {
    let many: Vec<AclEntry> = (0..MOST_ACLS + 7)
        .map(|i| AclEntry {
            name: format!("t{i}"),
            ..entry("User:a", AclPermission::Allow)
        })
        .collect();
    let read = Read {
        flag_users: vec![],
        supers: vec![],
        default_allow: false,
        stored: 0,
        acls: Ok(many),
        scram: vec![],
    };
    let v = assemble(read, &facts(&[], false), &StartFacts::default());
    assert_eq!(
        (v.acls.rows.len(), v.acls.more, v.authz.acls),
        (MOST_ACLS, 7, MOST_ACLS + 7)
    );
}
