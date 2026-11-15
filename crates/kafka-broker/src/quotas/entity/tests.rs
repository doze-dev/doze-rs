use super::*;

fn named(parts: &[(&str, Option<&str>)]) -> Named {
    parts
        .iter()
        .map(|(k, v)| ((*k).to_owned(), v.map(str::to_owned)))
        .collect()
}

fn set_op(key: &str, v: f64) -> (String, Option<f64>) {
    (key.to_owned(), Some(v))
}

fn alteration(parts: &[(&str, Option<&str>)], ops: Vec<(String, Option<f64>)>) -> Alteration {
    Alteration {
        entity: named(parts),
        ops,
    }
}

fn err_of(altered: &Altered, at: usize) -> Option<&str> {
    altered
        .results
        .get(at)
        .and_then(|(_, r)| r.as_ref().err().map(String::as_str))
}

#[test]
fn each_refusal_is_kafkas_message() {
    let none = BTreeMap::new();
    let cases: Vec<(Alteration, &str)> = vec![
        (alteration(&[], vec![]), "Invalid empty client quota entity"),
        (
            alteration(&[("group", Some("g"))], vec![]),
            "Unhandled client quota entity type: group",
        ),
        (
            alteration(&[("user", Some(""))], vec![]),
            "Empty user not supported",
        ),
        (
            alteration(&[("ip", Some("127.0.0.1")), ("user", Some("a"))], vec![]),
            "Invalid quota entity combination, IP entity shouldnot be combined with User or ClientId",
        ),
        (
            alteration(&[("user", Some("a"))], vec![set_op("bogus", 1.0)]),
            "Invalid configuration key bogus",
        ),
        (
            alteration(
                &[("user", Some("a"))],
                vec![set_op("producer_byte_rate", 0.0)],
            ),
            "Quota producer_byte_rate must be greater than 0",
        ),
        (
            alteration(
                &[("user", Some("a"))],
                vec![set_op("producer_byte_rate", f64::NAN)],
            ),
            "Quota producer_byte_rate must be a finite number",
        ),
        (
            alteration(
                &[("user", Some("a"))],
                vec![set_op("producer_byte_rate", 1.5)],
            ),
            "producer_byte_rate cannot be a fractional value.",
        ),
        (
            alteration(
                &[("user", Some("a"))],
                vec![set_op("consumer_byte_rate", 1e19)],
            ),
            "Proposed value for consumer_byte_rate is too large for a LONG.",
        ),
        (
            alteration(
                &[("ip", None)],
                vec![set_op("connection_creation_rate", 3e9)],
            ),
            "Proposed value for connection_creation_rate is too large for an INT.",
        ),
        (
            alteration(&[("ip", None)], vec![set_op("producer_byte_rate", 1.0)]),
            "Invalid configuration key producer_byte_rate",
        ),
        (
            alteration(
                &[("client-id", Some("c"))],
                vec![set_op("connection_creation_rate", 1.0)],
            ),
            "Invalid configuration key connection_creation_rate",
        ),
    ];
    for (a, want) in cases {
        let got = alter(&none, std::slice::from_ref(&a));
        assert_eq!(err_of(&got, 0), Some(want), "{a:?}");
        assert!(!got.changed && got.set.is_empty());
    }
}

#[test]
fn doubles_take_fractions_and_a_tiny_remainder_is_a_whole_number() {
    let none = BTreeMap::new();
    let ok = alter(
        &none,
        &[alteration(
            &[("user", None), ("client-id", Some("c"))],
            vec![
                set_op("request_percentage", 12.5),
                set_op("controller_mutation_rate", 0.5),
                set_op("producer_byte_rate", 1024.000_000_1),
            ],
        )],
    );
    assert_eq!(err_of(&ok, 0), None);
    let entity = QuotaEntity {
        user: Some(QuotaName::Default),
        client_id: Some(QuotaName::Named("c".into())),
        ip: None,
    };
    assert_eq!(ok.set.get(&entity).map(BTreeMap::len), Some(3));
}

#[test]
fn a_repeated_key_or_entity_is_ignored_as_a_duplicate_and_the_first_change_stands() {
    let none = BTreeMap::new();
    let alice = [("user", Some("alice"))];
    let twice = alter(
        &none,
        &[alteration(
            &alice,
            vec![
                set_op("producer_byte_rate", 1.0),
                set_op("producer_byte_rate", 2.0),
            ],
        )],
    );
    assert_eq!(
        err_of(&twice, 0),
        Some("Ignoring duplicate entity ClientQuotaEntity(entries={user=alice})")
    );
    assert!(twice.set.is_empty());
    let again = alter(
        &none,
        &[
            alteration(&alice, vec![set_op("producer_byte_rate", 1.0)]),
            alteration(&alice, vec![set_op("consumer_byte_rate", 2.0)]),
        ],
    );
    assert_eq!(again.results.len(), 1, "one answer per entity");
    assert!(err_of(&again, 0).is_some_and(|m| m.starts_with("Ignoring duplicate entity")));
    assert_eq!(again.set.values().next().map(BTreeMap::len), Some(1));
}

#[test]
fn a_removal_of_the_last_key_removes_the_entity_and_of_an_absent_key_changes_nothing() {
    let alice = QuotaEntity {
        user: Some(QuotaName::Named("alice".into())),
        client_id: None,
        ip: None,
    };
    let current = BTreeMap::from([(
        alice.clone(),
        BTreeMap::from([("producer_byte_rate".to_owned(), 10.0)]),
    )]);
    let absent = alter(
        &current,
        &[alteration(
            &[("user", Some("alice"))],
            vec![("consumer_byte_rate".into(), None)],
        )],
    );
    assert!(!absent.changed && err_of(&absent, 0).is_none());
    let gone = alter(
        &current,
        &[alteration(
            &[("user", Some("alice"))],
            vec![("producer_byte_rate".into(), None)],
        )],
    );
    assert!(gone.changed && !gone.set.contains_key(&alice));
}

#[test]
fn the_entity_text_lists_client_id_before_user_and_the_default_as_null() {
    assert_eq!(
        describe_named(&named(&[("user", None), ("client-id", Some("c1"))])),
        "ClientQuotaEntity(entries={client-id=c1, user=null})"
    );
}

fn stored() -> BTreeMap<QuotaEntity, BTreeMap<String, f64>> {
    let e =
        |user: Option<QuotaName>, client: Option<QuotaName>, ip: Option<QuotaName>| QuotaEntity {
            user,
            client_id: client,
            ip,
        };
    let n = |s: &str| Some(QuotaName::Named(s.to_owned()));
    let d = Some(QuotaName::Default);
    let v = BTreeMap::from([("request_percentage".to_owned(), 1.0)]);
    [
        e(n("alice"), None, None),
        e(n("alice"), n("c1"), None),
        e(d.clone(), None, None),
        e(d.clone(), n("c1"), None),
        e(None, d, None),
        e(None, None, n("10.0.0.1")),
    ]
    .into_iter()
    .map(|k| (k, v.clone()))
    .collect()
}

fn component(t: &str, m: i8, name: Option<&str>) -> Component {
    Component {
        entity_type: t.to_owned(),
        match_type: m,
        name: name.map(str::to_owned),
    }
}

fn matched(components: &[Component], strict: bool) -> Vec<Named> {
    let set = stored();
    describe(&set, components, strict)
        .unwrap()
        .into_iter()
        .map(|(e, _)| named_of(e))
        .collect()
}

#[test]
fn a_filter_matches_as_kafkas_image_does() {
    assert_eq!(
        matched(&[], false).len(),
        6,
        "no filter, not strict: everything"
    );
    assert!(matched(&[], true).is_empty(), "no filter, strict: nothing");
    let alice = matched(&[component("user", 0, Some("alice"))], false);
    assert_eq!(alice.len(), 2, "alice alone and alice with c1");
    let alice_strict = matched(&[component("user", 0, Some("alice"))], true);
    assert_eq!(alice_strict, vec![named(&[("user", Some("alice"))])]);
    let default_users = matched(&[component("user", 1, None)], false);
    assert_eq!(default_users.len(), 2);
    let users_with_clients = matched(
        &[component("user", 2, None), component("client-id", 2, None)],
        true,
    );
    assert_eq!(users_with_clients.len(), 2);
    let ips = matched(&[component("ip", 2, None)], false);
    assert_eq!(ips, vec![named(&[("ip", Some("10.0.0.1"))])]);
}

#[test]
fn a_filter_is_refused_in_kafkas_order_and_words() {
    let set = stored();
    let refused = |c: &[Component]| describe(&set, c, false).unwrap_err();
    assert_eq!(
        refused(&[component("", 0, Some("a"))]),
        (
            ResponseError::InvalidRequest,
            "Invalid empty entity type.".to_owned()
        )
    );
    assert_eq!(
        refused(&[component("user", 2, None), component("user", 0, Some("a"))]).1,
        "Entity type user cannot appear more than once in the filter."
    );
    assert_eq!(
        refused(&[component("group", 0, Some("a"))]),
        (
            ResponseError::UnsupportedVersion,
            "Unsupported entity type group".to_owned()
        )
    );
    assert_eq!(
        refused(&[component("user", 0, None)]).1,
        "Request specified MATCH_TYPE_EXACT, but set match string to null."
    );
    assert_eq!(
        refused(&[component("user", 1, Some("a"))]).1,
        "Request specified MATCH_TYPE_DEFAULT, but also specified a match string."
    );
    assert_eq!(
        refused(&[component("user", 2, Some("a"))]).1,
        "Request specified MATCH_TYPE_SPECIFIED, but also specified a match string."
    );
    assert_eq!(
        refused(&[component("user", 7, None)]).1,
        "Unknown match type 7"
    );
    assert_eq!(
        refused(&[component("ip", 2, None), component("user", 2, None)]).1,
        "Invalid entity filter component combination. IP filter component should not be used with user or clientId filter component."
    );
}

#[test]
fn an_address_must_be_one_or_resolve() {
    assert!(resolvable("127.0.0.1") && resolvable("::1") && resolvable("[::1]"));
    assert!(resolvable("localhost"));
    assert!(!resolvable("no-such-host.invalid"));
}
