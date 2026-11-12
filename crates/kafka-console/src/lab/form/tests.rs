use super::*;

fn fields(kind: LabKind, f: impl FnOnce(&mut LabFields)) -> LabFields {
    let mut x = blank(kind);
    f(&mut x);
    x
}

#[test]
fn the_forms_open_with_the_boards_defaults_and_make_the_boards_rules() {
    let slow = spec_of(
        LabKind::Slow,
        &fields(LabKind::Slow, |f| f.topic = "orders".into()),
    )
    .unwrap();
    assert_eq!(slow.name, "Slow produce to orders");
    assert_eq!(slow.ttl, Duration::from_secs(600));
    let fail = spec_of(
        LabKind::Fail,
        &fields(LabKind::Fail, |f| f.topic = "inventory.levels".into()),
    )
    .unwrap();
    assert_eq!(fail.effect, Effect::Fail { code: 7 });
    assert_eq!(fail.ttl, Duration::from_secs(300));
}

#[test]
fn each_wrong_field_gets_its_own_sentence() {
    let bad = fields(LabKind::Slow, |f| {
        f.api = "x".into();
        f.delay = "60001".into();
        f.jitter = "-1".into();
        f.share = "1".into();
        f.ends = "2".into();
        f.hits = "0".into();
        f.client = "a\nb".into();
    });
    let e = spec_of(LabKind::Slow, &bad).unwrap_err();
    let names: Vec<&str> = e.iter().map(|(k, _)| k.as_str()).collect();
    for want in ["api", "client", "share", "ends", "hits", "delay", "jitter"] {
        assert!(names.contains(&want), "{names:?}");
    }
    let code = fields(LabKind::Fail, |f| f.code = "99".into());
    assert_eq!(spec_of(LabKind::Fail, &code).unwrap_err()[0].0, "code");
}

#[test]
fn a_link_with_half_the_fields_opens_a_whole_form() {
    let f = filled(
        LabKind::Slow,
        LabFields {
            topic: "t".into(),
            ..LabFields::default()
        },
    );
    assert_eq!(
        (
            f.api.as_str(),
            f.delay.as_str(),
            f.share.as_str(),
            f.ends.as_str()
        ),
        ("produce", "400", "4", "10")
    );
    assert_eq!(f.topic, "t");
}

#[test]
fn the_preview_is_the_rule_and_what_the_wire_shows() {
    let spec = spec_of(
        LabKind::Slow,
        &fields(LabKind::Slow, |f| f.topic = "orders".into()),
    )
    .unwrap();
    let p = preview(&spec);
    assert_eq!(p.rule[0], ("doze".into(), "SlowTopic".into()));
    assert!(p.rule.contains(&("delay".into(), "400ms".into())));
    assert!(p.rule.contains(&("ttl".into(), "10 min".into())));
    assert!(p.wire.contains("400 ms"));
}

#[test]
fn text_is_cut_to_a_length_no_rule_needs() {
    let long = "x".repeat(5000);
    let f = fields_of(&[("topic".to_owned(), long)]);
    assert_eq!(f.topic.len(), 249);
}
