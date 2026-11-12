use super::*;

fn draft(name: &str, partitions: &str, retention: &str) -> Draft {
    Draft {
        name: name.to_owned(),
        partitions: partitions.to_owned(),
        template: Template::EventLog,
        cleanup: "delete".to_owned(),
        retention: retention.to_owned(),
    }
}

#[test]
fn the_starting_points_set_the_two_configs_the_form_draws() {
    assert_eq!(template_values(Template::EventLog), ("delete", "604800000"));
    assert_eq!(
        template_values(Template::Compacted),
        ("compact", "604800000")
    );
    assert_eq!(template_values(Template::ShortLived), ("delete", "3600000"));
    let fresh = Draft::fresh(6);
    assert_eq!((fresh.name.as_str(), fresh.partitions.as_str()), ("", "6"));
    assert_eq!(
        (fresh.cleanup.as_str(), fresh.retention.as_str()),
        ("delete", "604800000")
    );
}

#[test]
fn the_first_step_has_no_errors_until_the_person_moves_on() {
    let bad = draft("a b", "x", "y");
    assert_eq!(
        errors(&bad, Step::Name, None, Some(1000)),
        Errors::default()
    );
}

#[test]
fn moving_on_checks_the_steps_before_the_one_asked_for() {
    let ok = draft("orders.retry", "3", "604800000");
    assert_eq!(
        errors(&ok, Step::Config, Some(&Taken::Free), Some(1000)),
        Errors::default()
    );
    assert_eq!(
        errors(&ok, Step::Review, Some(&Taken::Free), Some(1000)),
        Errors::default()
    );
    let e = errors(
        &draft("a b", "0", "x"),
        Step::Review,
        Some(&Taken::Free),
        Some(1000),
    );
    assert!(e.name.unwrap().contains("U+0020"));
    assert_eq!(e.partitions.unwrap(), "A topic has at least 1 partition.");
    assert_eq!(e.retention.unwrap(), "A whole number is needed, such as 3.");
    // Retention is the Configuration step's: moving from step 1 to 2 does not judge it.
    assert!(
        errors(&draft("a", "1", "x"), Step::Config, None, None)
            .retention
            .is_none()
    );
    let taken = errors(&ok, Step::Config, Some(&Taken::Exists), None);
    assert_eq!(taken.name.unwrap(), "A topic has this name already.");
    let collides = errors(
        &ok,
        Step::Config,
        Some(&Taken::Collides("orders_retry".to_owned())),
        None,
    );
    assert_eq!(
        collides.name.unwrap(),
        "This name collides with the topic orders_retry."
    );
    assert!(
        errors(&draft("a", "1001", "1"), Step::Config, None, Some(1000))
            .partitions
            .unwrap()
            .contains("above this broker's limit")
    );
}

#[test]
fn a_form_lands_on_the_first_step_that_has_something_to_fix() {
    let none = Errors::default();
    assert_eq!(landing(Step::Review, &none), Step::Review);
    assert_eq!(landing(Step::Config, &none), Step::Config);
    assert_eq!(landing(Step::Name, &none), Step::Name);
    let name = Errors {
        name: Some("x".to_owned()),
        ..Errors::default()
    };
    assert_eq!(landing(Step::Review, &name), Step::Name);
    let parts = Errors {
        partitions: Some("x".to_owned()),
        ..Errors::default()
    };
    assert_eq!(landing(Step::Config, &parts), Step::Name);
    let ret = Errors {
        retention: Some("x".to_owned()),
        ..Errors::default()
    };
    assert_eq!(landing(Step::Review, &ret), Step::Config);
    let both = Errors {
        name: Some("x".to_owned()),
        retention: Some("y".to_owned()),
        ..Errors::default()
    };
    assert_eq!(landing(Step::Review, &both), Step::Name);
}
