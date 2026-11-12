use super::*;
use crate::fixtures_settings as larder;
use crate::gather::settings_edit::SettingsFields;
use crate::model::SettingsResult;

fn page() -> String {
    settings_page(&larder::page(), &Base::root()).into_string()
}

#[test]
fn the_page_has_its_six_sections_each_with_a_tab() {
    let p = page();
    for id in [
        "broker-configs",
        "version-profile",
        "intervals",
        "weight",
        "data",
        "flags",
    ] {
        assert!(p.contains(&format!("id=\"{id}\"")), "no section {id}");
        assert!(p.contains(&format!("href=\"#{id}\"")), "no tab {id}");
    }
    assert!(
        p.contains("<b>0</b> of the 5 are set at run time"),
        "{p:.2000}"
    );
    assert!(
        p.contains("Running <b>3 d 4 h</b> since Nov 6 18:02 UTC"),
        "{p:.2000}"
    );
}

#[test]
fn every_one_of_the_five_has_an_edit_button_that_opens_its_dialog() {
    let p = page();
    for key in [
        "log.segment.bytes",
        "log.index.interval.bytes",
        "message.max.bytes",
        "log.retention.bytes",
        "log.retention.ms",
    ] {
        assert!(
            p.contains(&format!(
                "href=\"/settings/config/edit?key={key}\" data-dialog"
            )),
            "no Edit for {key}"
        );
    }
}

#[test]
fn the_weight_says_where_it_cannot_measure_and_never_a_number_it_does_not_have() {
    let mut v = larder::page();
    v.weight.resident = None;
    v.weight.open_files = None;
    v.weight.binary = None;
    let p = settings_page(&v, &Base::root()).into_string();
    assert!(p.contains("Measured on Linux only"), "{p}");
    assert!(
        !p.contains("4.5<small>MB"),
        "a number the process did not read"
    );
    let measured = page();
    assert!(
        measured.contains("4.5<small>MB</small>") && measured.contains("limit 8 MB"),
        "{measured}"
    );
    assert!(measured.contains("the limit is 9.0 MB"), "{measured}");
}

#[test]
fn flags_that_differ_are_first_and_all_of_them_are_behind_show_all() {
    let p = page();
    assert!(
        p.contains(
            "<b>1</b> flag differs from its default: <span class=\"mono\">--data-dir</span>"
        ),
        "{p}"
    );
    assert!(p.contains("Show all 37 flags"), "{p}");
    assert!(p.contains("<span class=\"pill\">set</span>"));
    assert!(p.contains("also dynamic"));
}

#[test]
fn a_broker_that_does_not_know_its_flags_says_so_and_invents_none() {
    let mut v = larder::page();
    v.flags.clear();
    v.intervals.rows.clear();
    let p = settings_page(&v, &Base::root()).into_string();
    assert!(p.contains("did not say how it was started"), "{p}");
    assert!(!p.contains("flags</summary>"), "no flags table: {p}");
}

#[test]
fn hostile_text_in_a_value_or_a_name_is_escaped_everywhere_it_is_shown() {
    let mut v = larder::page();
    v.data.dir = "<script>alert(1)</script>".to_owned();
    v.flags[0].value = "\"><img src=x onerror=alert(1)>".to_owned();
    v.configs.rows[0].source_note = "<b>x</b>".to_owned();
    let p = settings_page(&v, &Base::root()).into_string();
    assert!(
        !p.contains("<script>alert(1)</script>") && !p.contains("<img src=x"),
        "{p}"
    );
    assert!(p.contains("&lt;script&gt;"));
}

#[test]
fn the_edit_dialog_is_one_form_with_a_check_a_save_and_the_call() {
    let v = larder::form("log.retention.ms", "all").unwrap();
    let d = edit_dialog(&v, &Base::root()).into_string();
    assert!(
        d.contains("role=\"dialog\"") && d.contains("aria-labelledby=\"broker-config-edit-title\""),
        "{d}"
    );
    assert!(d.contains("<h2 id=\"broker-config-edit-title\">Edit log.retention.ms</h2>"));
    assert!(
        d.contains("name=\"check\" value=\"1\"")
            && d.contains("data-op=\"IncrementalAlterConfigs\"")
    );
    assert!(d.contains("action=\"/settings/config/edit\""));
    assert!(d.contains("<option value=\"log.retention.ms\" selected>"));
    assert!(d.contains("ms = 3 d"), "{d}");
    assert!(d.contains("BROKER &quot;&quot; (all brokers)"), "{d}");
    // An overlay over the Settings page: the scrim and Cancel lead back to it, and a script takes the Cancel over.
    assert!(
        d.contains("class=\"ovl ovl-dialog root\" data-overlay data-close=\"/settings\""),
        "{d}"
    );
    assert!(
        d.contains("<a class=\"btn\" href=\"/settings\" data-dialog-close>Cancel</a>"),
        "{d}"
    );
}

#[test]
fn a_saved_change_and_a_refusal_say_what_the_broker_said() {
    let f = SettingsFields {
        key: "log.retention.ms",
        was: "log.retention.ms",
        scope: "all",
        value: "259200000",
    };
    let SettingsResult::Saved(s) = larder::save(f, false) else {
        panic!()
    };
    let p = settings_saved(&s, &Base::root()).into_string();
    assert!(
        p.contains("<b>Saved.</b>") && p.contains("259,200,000") && p.contains("(3 d)"),
        "{p}"
    );
    assert!(p.contains("-&gt; 200, no error"), "{p}");
    let f = SettingsFields {
        key: "num.partitions",
        was: "num.partitions",
        scope: "all",
        value: "3",
    };
    let SettingsResult::Refused(r) = larder::save(f, true) else {
        panic!()
    };
    let p = settings_refused(&r, &Base::root()).into_string();
    assert!(
        p.contains("INVALID_REQUEST (42)")
            && p.contains("Cannot update these configs dynamically: [num.partitions]"),
        "{p}"
    );
    assert!(p.contains("--default-partitions") && p.contains("kafka-configs.sh"));
}
