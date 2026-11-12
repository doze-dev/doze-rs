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
        listener_beyond_loopback: listener_beyond,
        console_beyond_loopback: console_beyond,
        console_addr: "127.0.0.1:9093",
        listener: "127.0.0.1:9092",
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
        (s(false, false, true, false, true), [false, false, false]),
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
            "Console served beyond loopback with SASL or TLS on",
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
fn the_posture_is_the_three_switches() {
    assert_eq!(posture(false, 0, true), Posture::Open);
    assert_eq!(posture(false, 1, true), Posture::AclsOnly);
    assert_eq!(posture(false, 0, false), Posture::AclsOnly);
    assert_eq!(posture(true, 0, true), Posture::SignIn);
    assert_eq!(posture(true, 8, false), Posture::SignIn);
    assert_eq!(Posture::SignIn.word(), "Sign-in required");
    assert_eq!(Posture::Open.word(), "Open");
}

#[test]
fn the_counts_read_as_the_board_writes_them() {
    assert_eq!(users_line(1, 3), "1 from flags, 3 stored");
    assert_eq!(acl_split(7, 1), "7 allow, 1 deny");
    let sub_text = format!(
        "{:?}",
        sub(
            Posture::SignIn,
            "x",
            &["SCRAM-SHA-256".into(), "SCRAM-SHA-512".into()],
            true
        )
    );
    assert!(
        sub_text.contains("SCRAM-SHA-256 or SCRAM-SHA-512 over TLS"),
        "{sub_text}"
    );
    let open = format!("{:?}", sub(Posture::Open, "127.0.0.1:9092", &[], false));
    assert!(
        open.contains("127.0.0.1:9092") && open.contains("nobody signs in"),
        "{open}"
    );
}
