//! The permission warning, and the redaction of passwords from every source.

use super::*;

#[cfg(unix)]
#[test]
fn a_file_others_can_read_is_warned_about_and_a_private_one_is_not() {
    use std::os::unix::fs::PermissionsExt;
    let dir = Scratch::new("mode");
    let file = dir.file("users", "alice:pw\n");
    assert_eq!(
        crate::cli::sasl::permission_warning(std::path::Path::new(&file)),
        None
    );
    for mode in [0o640, 0o604, 0o644, 0o666, 0o660] {
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(mode)).unwrap();
        let warning = crate::cli::sasl::permission_warning(std::path::Path::new(&file)).unwrap();
        assert!(
            warning.contains(&file) && warning.contains("chmod 600"),
            "{mode:o}: {warning}"
        );
        assert!(
            !warning.contains("alice") && !warning.contains("pw\n"),
            "{warning}"
        );
    }
    for mode in [0o600, 0o400, 0o700] {
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(mode)).unwrap();
        assert_eq!(
            crate::cli::sasl::permission_warning(std::path::Path::new(&file)),
            None,
            "{mode:o}"
        );
    }
}

#[test]
fn no_password_from_any_source_is_in_a_debug_output() {
    let dir = Scratch::new("redact");
    let file = dir.file("users", "fileuser:file-planted-pw\n");
    let flags = [
        &MECHS[..],
        &[
            "--sasl-user",
            "flaguser:flag-planted-pw",
            "--sasl-users-file",
            &file,
        ],
    ]
    .concat();
    let env = "envuser:env-planted-pw";
    let parsed = args(&flags).unwrap();
    let set = parsed
        .settings_in(&|n| (n == "DOZE_KAFKA_SASL_USERS").then(|| OsString::from(env)))
        .unwrap();
    let shown = format!("{parsed:?} {set:?} {:#?}", set.server.sasl);
    for user in ["fileuser", "flaguser", "envuser"] {
        assert!(shown.contains(user), "{shown}");
    }
    assert!(
        !shown.contains("planted") && !shown.contains("-pw"),
        "{shown}"
    );
}

#[test]
fn the_failed_login_delay_is_kafkas_hundred_milliseconds_unless_the_flag_says_otherwise() {
    // SEC-192.
    let delay = |flags: &[&str]| settings(flags, None).unwrap().server.timeouts.failed_login;
    assert_eq!(delay(&[]), std::time::Duration::from_millis(100));
    assert_eq!(
        delay(&["--sasl-failed-login-delay", "2s"]),
        std::time::Duration::from_secs(2)
    );
    assert_eq!(
        delay(&["--sasl-failed-login-delay", "0"]),
        std::time::Duration::ZERO
    );
    assert!(args(&["--sasl-failed-login-delay", "soon"]).is_err());
}

#[test]
fn a_failed_login_delay_over_ten_seconds_is_refused_at_start_and_ten_seconds_is_not() {
    // SEC-236.
    let delay = |flags: &[&str]| settings(flags, None).unwrap().server.timeouts.failed_login;
    assert_eq!(
        delay(&["--sasl-failed-login-delay", "10s"]),
        std::time::Duration::from_secs(10)
    );
    for too_long in ["10001ms", "11s", "1h", "1m"] {
        let refused = args(&["--sasl-failed-login-delay", too_long])
            .unwrap_err()
            .to_string();
        assert!(
            refused.contains("a failed login may be made to wait"),
            "{too_long}: {refused}"
        );
    }
}
