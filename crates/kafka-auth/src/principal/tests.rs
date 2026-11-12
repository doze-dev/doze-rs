use super::*;

#[test]
fn a_principal_is_written_type_colon_name() {
    assert_eq!(Principal::user("alice").as_str(), "User:alice");
    assert_eq!(Principal::user("alice").to_string(), "User:alice");
    assert_eq!(Principal::anonymous().as_str(), "User:ANONYMOUS");
    assert_ne!(Principal::user("alice"), Principal::user("Alice"));
}

#[test]
fn mechanism_names_are_exact_and_case_sensitive() {
    for m in Mechanism::ALL {
        assert_eq!(Mechanism::from_name(m.name()), Some(m));
        assert_eq!(m.to_string(), m.name());
    }
    assert_eq!(Mechanism::Plain.name(), "PLAIN");
    assert_eq!(Mechanism::ScramSha256.name(), "SCRAM-SHA-256");
    assert_eq!(Mechanism::ScramSha512.name(), "SCRAM-SHA-512");
    for wrong in [
        "plain",
        "Plain",
        "",
        "BOGUS",
        "SCRAM-SHA-1",
        "scram-sha-256",
        " PLAIN",
        "PLAIN ",
    ] {
        assert_eq!(Mechanism::from_name(wrong), None, "{wrong:?}");
    }
}

#[test]
fn the_consoles_principal_is_user_dunder_console_and_only_that_name_is_it() {
    assert_eq!(Principal::console().as_str(), "User:__console");
    assert_eq!(Principal::console(), Principal::user(CONSOLE_USER));
    assert!(Principal::is_console("User:__console"));
    for other in [
        "User:__console2",
        "User:_console",
        "User:__Console",
        "Group:__console",
        "__console",
        "User:",
        "",
        "User:__console ",
        "user:__console",
    ] {
        assert!(!Principal::is_console(other), "{other:?}");
    }
}
