use super::*;

fn s(
    sasl: bool,
    tls: bool,
    allow: bool,
    listener_beyond: bool,
    console_beyond: bool,
) -> Switches<'static> {
    Switches {
        sasl,
        tls,
        default_allow: allow,
        acls_stored: false,
        listener_beyond_loopback: listener_beyond,
        console_beyond_loopback: console_beyond,
        // A guarded broker starts a console beyond loopback only with --console-unprotected.
        console_unprotected: console_beyond,
        console_addr: "127.0.0.1:9093",
    }
}

fn holds(w: &[AccessWarning]) -> Vec<bool> {
    w.iter().map(|x| x.holds).collect()
}

#[test]
fn each_warning_holds_exactly_when_the_broker_would_say_it_at_start() {
    // (sasl, tls, default allow, listener beyond loopback, console beyond loopback) -> the three.
    let table = [
        (s(false, false, true, false, false), [false, false, false]),
        (s(true, true, false, false, false), [false, false, false]),
        (s(true, false, true, true, false), [false, true, true]),
        (s(true, false, false, true, false), [false, true, false]),
        (s(true, true, true, true, false), [false, false, true]),
        (s(false, true, true, false, true), [true, false, false]),
        (s(true, false, false, false, true), [true, false, false]),
        // Kafka on loopback and the console beyond it: the console is the wider door.
        (s(false, false, true, false, true), [true, false, false]),
        // Nothing guards the broker and it listens beyond loopback too: the console is no wider.
        (s(false, false, true, true, true), [false, false, false]),
        // ACLs in force (deny) are a guard on their own.
        (s(false, false, false, true, true), [true, false, false]),
    ];
    for (switches, want) in table {
        assert_eq!(holds(&warnings(&switches)), want, "{switches:?}");
    }
}

#[test]
fn the_three_warnings_say_what_the_board_says_in_both_states() {
    let quiet = warnings(&s(false, false, true, false, false));
    let titles: Vec<String> = quiet
        .iter()
        .map(|w| {
            w.title
                .iter()
                .map(|p| match p {
                    Span::Text(t) | Span::Mono(t) | Span::Strong(t) => t.as_str(),
                })
                .collect()
        })
        .collect();
    assert_eq!(
        titles,
        [
            "Console served beyond loopback, a wider door than the broker",
            "SASL without TLS beyond loopback",
            "SASL on with --acl-default allow",
        ]
    );
    let loud = warnings(&s(true, false, true, true, true));
    assert!(format!("{:?}", loud[0].because).contains("--console-unprotected"));
    assert!(format!("{:?}", loud[1].because).contains("PLAIN password"));
    assert!(format!("{:?}", loud[2].because).contains("manage users and ACLs"));
}

#[test]
fn stored_acls_alone_make_a_console_beyond_loopback_the_wider_door() {
    let mut sw = s(false, false, true, true, true);
    assert!(!warnings(&sw)[0].holds);
    sw.acls_stored = true;
    assert!(warnings(&sw)[0].holds);
}

#[test]
fn acls_made_after_an_open_start_shut_the_console_to_other_machines_and_the_warning_says_so() {
    // An open broker let the console start beyond loopback without --console-unprotected; then an ACL was made.
    let mut sw = s(false, false, true, true, true);
    sw.console_unprotected = false;
    assert!(!warnings(&sw)[0].holds);
    sw.acls_stored = true;
    let one = &warnings(&sw)[0];
    assert!(!one.holds, "the door is shut, not wider");
    let said = format!("{:?}", one.because);
    assert!(
        said.contains("now has ACLs, so it answers this machine only"),
        "{said}"
    );
    assert!(said.contains("--console-unprotected"), "{said}");
}
