use super::*;

fn answered(ms: u64) -> Dial {
    Dial {
        answered: true,
        ms,
        why: String::new(),
    }
}

fn failed(why: &str) -> Dial {
    Dial {
        answered: false,
        ms: 3,
        why: why.to_owned(),
    }
}

fn good() -> Measured {
    Measured {
        listen: "127.0.0.1:9092".into(),
        metadata_version: 12,
        advertised: "127.0.0.1:9092".into(),
        tls: false,
        listener: answered(2),
        metadata: Named::Broker("127.0.0.1:9092".into()),
        advertised_dial: Some(answered(2)),
    }
}

fn text(r: &Rule) -> String {
    r.detail
        .iter()
        .map(|s| match s {
            Span::Text(t) | Span::Mono(t) | Span::Strong(t) => t.as_str(),
        })
        .collect()
}

#[test]
fn an_address_that_answers_is_reachable_in_the_boards_words() {
    let v = judge(&good());
    assert_eq!(v.verdict, Verdict::Reachable);
    assert!(v.same);
    let titles: Vec<_> = v.rules.iter().map(|r| r.title.as_str()).collect();
    assert_eq!(
        titles,
        [
            "The listener answers",
            "It says where clients should reconnect",
            "That address is reachable"
        ]
    );
    assert_eq!(
        text(&v.rules[0]),
        "Dialled 127.0.0.1:9092 and asked ApiVersions"
    );
    assert_eq!(
        text(&v.rules[1]),
        "Metadata names this broker at 127.0.0.1:9092: the advertised host and port equal the listener"
    );
    assert_eq!(
        text(&v.rules[2]),
        "Dialled the advertised address back and got an answer"
    );
    assert_eq!(
        v.rules.iter().map(|r| r.value.as_str()).collect::<Vec<_>>(),
        ["2 ms", "Metadata", "2 ms"]
    );
    assert!(v.rules.iter().all(|r| r.ok));
}

#[test]
fn a_wrong_advertised_address_is_the_boards_bad_case() {
    let mut m = good();
    m.advertised = "broker.internal:9092".into();
    m.metadata = Named::Broker("broker.internal:9092".into());
    m.advertised_dial = Some(failed("no such host"));
    let v = judge(&m);
    assert_eq!(v.verdict, Verdict::NotReachable);
    assert!(!v.same);
    assert!(v.rules[0].ok && v.rules[1].ok && !v.rules[2].ok);
    assert_eq!(v.rules[2].title, "That address is not reachable");
    assert_eq!(
        text(&v.rules[2]),
        "Dialled broker.internal:9092: no such host"
    );
    assert_eq!(v.rules[2].value, "failed");
    assert_eq!(
        text(&v.rules[1]),
        "Metadata names this broker at broker.internal:9092, not at the address it listens on (127.0.0.1:9092)"
    );
}

#[test]
fn a_listener_that_does_not_answer_fails_the_first_rule() {
    let mut m = good();
    m.listener = failed("connection refused");
    let v = judge(&m);
    assert_eq!(v.verdict, Verdict::NotReachable);
    assert_eq!(v.rules[0].title, "The listener does not answer");
    assert_eq!(
        text(&v.rules[0]),
        "Dialled 127.0.0.1:9092: connection refused"
    );
}

#[test]
fn metadata_that_names_something_else_or_nothing_fails_the_second() {
    let mut m = good();
    m.metadata = Named::Broker("10.0.0.5:9092".into());
    let v = judge(&m);
    assert!(!v.rules[1].ok);
    assert_eq!(
        text(&v.rules[1]),
        "Metadata names this broker at 10.0.0.5:9092, not 127.0.0.1:9092"
    );
    m.metadata = Named::Nothing("it named no broker".into());
    let v = judge(&m);
    assert!(!v.rules[1].ok);
    assert_eq!(text(&v.rules[1]), "Metadata: it named no broker");
}

#[test]
fn a_dial_that_did_not_finish_claims_nothing() {
    let mut m = good();
    m.advertised_dial = None;
    let v = judge(&m);
    assert_eq!(v.verdict, Verdict::NotChecked);
    assert!(v.rules.is_empty());
}

#[test]
fn tls_is_only_dialled() {
    let mut m = good();
    m.tls = true;
    assert_eq!(
        text(&judge(&m).rules[0]),
        "Dialled 127.0.0.1:9092 and connected (it serves TLS, so ApiVersions was not asked)"
    );
}

#[test]
fn a_rule_is_ok_exactly_when_the_verdict_can_be_reachable() {
    for listener in [true, false] {
        for named in [true, false] {
            for dial in [Some(true), Some(false), None] {
                let mut m = good();
                m.listener = if listener { answered(1) } else { failed("x") };
                m.metadata = if named {
                    Named::Broker(m.advertised.clone())
                } else {
                    Named::Nothing("n".into())
                };
                m.advertised_dial = dial.map(|d| if d { answered(1) } else { failed("y") });
                let v = judge(&m);
                let want = match dial {
                    None => Verdict::NotChecked,
                    Some(d) if d && listener && named => Verdict::Reachable,
                    Some(_) => Verdict::NotReachable,
                };
                assert_eq!(v.verdict, want, "{listener} {named} {dial:?}");
            }
        }
    }
}
