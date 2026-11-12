//! Where the bootstrap users come from: `--sasl-user`, `--sasl-users-file` and `DOZE_KAFKA_SASL_USERS`.

use std::ffi::OsString;
use std::path::PathBuf;

use super::*;

const MECHS: [&str; 2] = ["--sasl-mechanisms", "PLAIN,SCRAM-SHA-256"];

/// A directory of this test's own, removed when it is dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("doze-kafka-users-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn file(&self, name: &str, content: &str) -> String {
        let path = self.0.join(name);
        std::fs::write(&path, content).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        path.display().to_string()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The settings of `flags` with `env` as `DOZE_KAFKA_SASL_USERS`, or the refusal in words.
fn settings(flags: &[&str], env: Option<&str>) -> Result<crate::cli::Settings, String> {
    let env = env.map(OsString::from);
    args(flags)
        .unwrap()
        .settings_in(&|name| {
            (name == "DOZE_KAFKA_SASL_USERS")
                .then(|| env.clone())
                .flatten()
        })
        .map_err(|e| format!("{e:#}"))
}

fn plain_ok(set: &crate::cli::Settings, user: &str, password: &str) -> bool {
    let auth = set.server.sasl.as_ref().unwrap();
    auth.plain(format!("\0{user}\0{password}").as_bytes())
        .is_ok()
}

#[test]
fn a_users_file_holds_one_user_and_password_a_line_split_at_the_first_colon() {
    let dir = Scratch::new("file");
    let file = dir.file(
        "users",
        "# the admin\nadmin:admin-secret\n\n   \n  # indented comment\nalice:a:b:c\r\nbob: spaced \n",
    );
    let set = settings(&[&MECHS[..], &["--sasl-users-file", &file]].concat(), None).unwrap();
    assert_eq!(set.server.sasl.as_ref().unwrap().user_count(), 3);
    assert!(plain_ok(&set, "admin", "admin-secret"));
    // The rest of the line is the password, colons and all; a CRLF ending is not part of it.
    assert!(plain_ok(&set, "alice", "a:b:c"));
    // Nothing else is trimmed: a space is a character.
    assert!(plain_ok(&set, "bob", " spaced "));
}

#[test]
fn an_environment_list_is_comma_separated_user_and_password() {
    let env = "admin:admin-secret, alice:a:b:c ,,bob:p";
    let set = settings(&MECHS, Some(env)).unwrap();
    assert_eq!(set.server.sasl.as_ref().unwrap().user_count(), 3);
    assert!(plain_ok(&set, "admin", "admin-secret"));
    assert!(plain_ok(&set, "alice", "a:b:c"));
    assert!(plain_ok(&set, "bob", "p"));
    // A password with a comma is not expressible there: the part after the comma is another entry.
    let set = settings(&MECHS, Some("carol:one,two:three")).unwrap();
    assert!(plain_ok(&set, "carol", "one") && plain_ok(&set, "two", "three"));
    assert!(!plain_ok(&set, "carol", "one,two:three"));
}

#[test]
fn an_empty_variable_is_the_same_as_none() {
    let said = settings(&MECHS, Some("")).unwrap_err();
    assert!(said.contains("no credentials are configured"), "{said}");
}

#[test]
fn the_three_sources_add_up_in_the_order_flag_file_environment() {
    let dir = Scratch::new("all");
    let file = dir.file("users", "fromfile:f\n");
    let flags = [
        &MECHS[..],
        &["--sasl-user", "fromflag:g", "--sasl-users-file", &file],
    ]
    .concat();
    let set = settings(&flags, Some("fromenv:e")).unwrap();
    assert_eq!(set.server.sasl.as_ref().unwrap().user_count(), 3);
    for (u, p) in [("fromflag", "g"), ("fromfile", "f"), ("fromenv", "e")] {
        assert!(plain_ok(&set, u, p), "{u}");
    }
    // The broker shares the users the authenticator logs in.
    assert_eq!(set.broker.users.bootstrap_len(), 3);
    assert!(std::sync::Arc::ptr_eq(
        set.server.sasl.as_ref().unwrap().users(),
        &set.broker.users
    ));
}

#[test]
fn a_user_given_twice_across_sources_is_refused_naming_the_user_and_never_a_password() {
    let dir = Scratch::new("dup");
    let file = dir.file("users", "alice:file-planted\n");
    for (flags, env) in [
        (vec!["--sasl-user", "alice:flag-planted"], None),
        (vec![], Some("alice:env-planted")),
    ] {
        let flags = [&MECHS[..], &["--sasl-users-file", &file], &flags[..]].concat();
        let said = settings(&flags, env).unwrap_err();
        assert!(said.contains("\"alice\" is given more than once"), "{said}");
        assert!(!said.contains("planted"), "{said}");
    }
    // Twice in the same file, and in the same list.
    let twice = dir.file("twice", "bob:one-planted\nbob:two-planted\n");
    let said = settings(&[&MECHS[..], &["--sasl-users-file", &twice]].concat(), None).unwrap_err();
    assert!(said.contains("\"bob\" is given more than once") && !said.contains("planted"));
    let said = settings(&MECHS, Some("bob:one-planted,bob:two-planted")).unwrap_err();
    assert!(said.contains("\"bob\" is given more than once") && !said.contains("planted"));
}

#[test]
fn a_bad_line_is_named_by_its_number_and_its_content_is_not_repeated() {
    let dir = Scratch::new("bad");
    let cases = [
        (
            "alice:ok\nnocolon-planted\n",
            "line 2: not a valid entry (expected user:password)",
        ),
        (
            "# c\n\n:empty-user-planted\n",
            "line 3: SASL credential has an empty username",
        ),
        (
            "alice:\n",
            "line 1: SASL user \"alice\" has an empty password",
        ),
        (
            "bob:caf\u{e9}-planted\n",
            "line 1: SASL user \"bob\" has a password that is not ASCII",
        ),
    ];
    for (i, (text, why)) in cases.into_iter().enumerate() {
        let file = dir.file(&format!("u{i}"), text);
        let said =
            settings(&[&MECHS[..], &["--sasl-users-file", &file]].concat(), None).unwrap_err();
        assert!(
            said.contains("--sasl-users-file") && said.contains(why),
            "{text:?}: {said}"
        );
        assert!(!said.contains("planted") && !said.contains("caf"), "{said}");
    }
    let said = settings(&MECHS, Some("alice:ok,nocolon-planted")).unwrap_err();
    assert!(
        said.contains("DOZE_KAFKA_SASL_USERS: entry 2: not a valid entry (expected user:password)"),
        "{said}"
    );
    assert!(!said.contains("planted"), "{said}");
}

#[test]
fn a_file_that_cannot_be_read_is_refused_without_a_guess() {
    let dir = Scratch::new("unreadable");
    let missing = dir.0.join("nope").display().to_string();
    let said = settings(
        &[&MECHS[..], &["--sasl-users-file", &missing]].concat(),
        None,
    )
    .unwrap_err();
    assert!(
        said.contains("--sasl-users-file") && said.contains("cannot be read"),
        "{said}"
    );
    let binary = dir.0.join("binary");
    std::fs::write(&binary, [0xff, 0xfe, b'a', b':', b'b']).unwrap();
    let said = settings(
        &[
            &MECHS[..],
            &["--sasl-users-file", &binary.display().to_string()],
        ]
        .concat(),
        None,
    )
    .unwrap_err();
    assert!(said.contains("is not valid UTF-8"), "{said}");
}

#[test]
fn users_from_the_file_or_the_environment_without_a_mechanism_are_refused() {
    let dir = Scratch::new("nomech");
    let file = dir.file("users", "alice:pw\n");
    let said = settings(&["--sasl-users-file", &file], None).unwrap_err();
    assert!(said.contains("no mechanism is enabled"), "{said}");
    let said = settings(&[], Some("alice:pw")).unwrap_err();
    assert!(said.contains("no mechanism is enabled"), "{said}");
    let said = settings(&["--sasl-allow-empty-users"], None).unwrap_err();
    assert!(
        said.contains("--sasl-allow-empty-users") && said.contains("--sasl-mechanisms"),
        "{said}"
    );
    // The variable alone, empty, is nothing.
    assert!(settings(&[], Some("")).unwrap().server.sasl.is_none());
}

#[test]
fn mechanisms_without_a_user_are_refused_unless_a_stored_user_may_exist_or_it_was_meant() {
    let dir = Scratch::new("empty");
    let data = dir.0.join("data");
    std::fs::create_dir_all(&data).unwrap();
    let data_flag = data.display().to_string();
    let base = [&MECHS[..], &["--data-dir", &data_flag]].concat();
    // A data directory with no database has no stored user: refused now, before anything is created.
    let said = settings(&base, None).unwrap_err();
    assert!(
        said.contains("SASL is enabled but no credentials are configured"),
        "{said}"
    );
    assert!(
        said.contains("--sasl-users-file") && said.contains("--sasl-allow-empty-users"),
        "{said}"
    );
    assert!(!data.join("meta.redb").exists());
    // On purpose: the first user is made over the API by a super user.
    let flags = [&base[..], &["--sasl-allow-empty-users"]].concat();
    let set = settings(&flags, None).unwrap();
    assert!(set.allow_empty_users);
    assert_eq!(set.server.sasl.as_ref().unwrap().user_count(), 0);
    // A database that may hold stored users: the start goes on, and the broker looks once it has opened it.
    std::fs::write(data.join("meta.redb"), b"x").unwrap();
    let set = settings(&base, None).unwrap();
    assert!(!set.allow_empty_users);
    assert!(
        set.server
            .sasl
            .as_ref()
            .unwrap()
            .ensure_users(false)
            .is_err()
    );
}

mod more;
