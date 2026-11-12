use super::*;
use crate::model::SettingsConfigRow;

fn plain(lines: &[Vec<(Tok, String)>]) -> Vec<String> {
    lines
        .iter()
        .map(|l| l.iter().map(|(_, t)| t.as_str()).collect())
        .collect()
}

#[test]
fn the_call_is_the_api_the_broker_resource_and_one_set_as_the_boards_draw_it() {
    let all = call_lines(1, SettingsScope::All, 1, "log.retention.ms", "259200000");
    assert_eq!(
        plain(&all),
        [
            "IncrementalAlterConfigs v1",
            "  BROKER \"\" (all brokers)",
            "  SET log.retention.ms = 259200000"
        ]
    );
    assert_eq!(all[0][0].0, Tok::Keyword);
    let this = call_lines(1, SettingsScope::This, 7, "log.retention.ms", "5");
    assert_eq!(plain(&this)[1], "  BROKER \"7\" (this broker)");
}

#[test]
fn what_was_typed_cannot_start_a_line_of_its_own() {
    let call = call_lines(1, SettingsScope::All, 1, "k", "1\n  evil  SET  2");
    assert_eq!(call.len(), 3);
    assert!(plain(&call)[2].contains("\\n"), "{call:?}");
}

#[test]
fn a_scope_names_the_resource_the_broker_keeps_the_value_under() {
    assert_eq!(resource_name(SettingsScope::All, 1), "");
    assert_eq!(resource_name(SettingsScope::This, 7), "7");
}

fn row(name: &str, value: &str) -> SettingsConfigRow {
    SettingsConfigRow {
        name: name.into(),
        what: String::new(),
        value: value.into(),
        gloss: None,
        source: crate::model::ConfigSource::Default,
        source_note: "Kafka's default".into(),
        takes_effect: String::new(),
    }
}

#[test]
fn the_dialog_teaches_what_is_so_now_and_what_the_typed_value_comes_to() {
    let configs = BrokerConfigs {
        rows: vec![row("log.retention.ms", "-1")],
        fixed: vec![],
    };
    let who = Followers::default();
    let w = World {
        node: 1,
        retention_secs: Some(300),
        configs: &configs,
        who: &who,
        version: 1,
    };
    let v = view_of(
        &w,
        "log.retention.ms",
        SettingsScope::All,
        "259200000",
        (None, None),
    );
    assert_eq!(v.unit.as_deref(), Some("ms = 3 d"));
    assert_eq!(v.keys.len(), 5);
    assert!(format!("{:?}", v.teach.now).contains("-1"));
    assert!(
        format!("{:?}", v.teach.takes_effect).contains("at the next retention pass, every 5 m")
    );
    assert_eq!(v.teach.allowed.dim, "-1 keeps them for ever.");
    assert_eq!(v.error, None);
}
