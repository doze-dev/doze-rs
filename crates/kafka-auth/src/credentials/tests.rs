use super::*;

fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter()
        .map(|(u, p)| ((*u).to_owned(), (*p).to_owned()))
        .collect()
}

#[test]
fn users_are_found_by_exact_name() {
    let c = Credentials::new(pairs(&[("alice", "alice-secret"), ("bob", "b:ob")])).unwrap();
    assert_eq!(c.len(), 2);
    assert_eq!(c.password(b"alice").unwrap().expose(), b"alice-secret");
    assert_eq!(c.password(b"bob").unwrap().expose(), b"b:ob");
    for unknown in [&b"ALICE"[..], b"alic", b"alice ", b"", &[0xff, 0xfe]] {
        assert!(c.password(unknown).is_none(), "{unknown:?}");
    }
}

#[test]
fn what_cannot_be_a_login_is_refused_with_words_that_leave_out_the_password() {
    let cases = [
        (pairs(&[("", "x")]), "empty username"),
        (pairs(&[("al\0ice", "x")]), "NUL"),
        (pairs(&[("alice", "")]), "empty password"),
        (pairs(&[("alice", "caf\u{e9}")]), "not ASCII"),
        (pairs(&[("alice", "a\0b")]), "not ASCII"),
        (
            pairs(&[("alice", "one"), ("alice", "two")]),
            "more than once",
        ),
    ];
    for (input, why) in cases {
        let e = Credentials::new(input).unwrap_err().to_string();
        assert!(e.contains(why), "{e}");
        for password in ["one", "two", "caf\u{e9}", "a\0b"] {
            assert!(!e.contains(password), "{e} shows a password");
        }
    }
}

#[test]
fn no_users_is_a_valid_set_and_is_empty() {
    let c = Credentials::new(Vec::new()).unwrap();
    assert!(c.is_empty());
    assert!(c.password(b"alice").is_none());
}

#[test]
fn debug_output_names_the_users_and_never_a_password() {
    let c = Credentials::new(pairs(&[
        ("alice", "hunter2-planted"),
        ("bob", "zebra-planted"),
    ]))
    .unwrap();
    let shown = format!("{c:?} {:#?} {:?}", c, c.password(b"alice").unwrap());
    assert!(shown.contains("alice") && shown.contains("bob"));
    assert!(shown.contains("redacted"));
    for planted in ["hunter2", "zebra", "planted"] {
        assert!(!shown.contains(planted), "{shown}");
    }
}
