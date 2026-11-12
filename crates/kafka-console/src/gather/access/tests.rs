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
