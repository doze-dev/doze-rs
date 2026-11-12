//! The MultiSelect: markup, roles, escaping, and that the script defines what the markup binds.

use super::gallery::HOSTILE;
use super::*;

const COLOURS: [Choice<'static>; 3] = [
    Choice::new("r", "Red"),
    Choice::new("g", "Green"),
    Choice::new("b", "Blue"),
];

pub(super) fn multi(values: &[&str]) -> String {
    let values: Vec<String> = values.iter().map(ToString::to_string).collect();
    multiselect(&MultiSelect {
        placeholder: "All colours",
        noun: "colours",
        auto_submit: true,
        ..MultiSelect::new("colours", "colour", "Colours", &COLOURS, &values)
    })
    .into_string()
}

#[test]
fn a_multiselect_is_checkboxes_in_a_details_underneath_and_a_multiselectable_listbox_on_top() {
    let m = multi(&["g", "b"]);
    // The no-script control: one checkbox per option, the chosen ones checked, all with the one repeated name.
    assert!(
        m.contains(r#"<details class="dz-native dz-multi-plain" id="colours">"#),
        "{m}"
    );
    assert!(
        m.contains(r#"<input type="checkbox" name="colour" value="r" id="colours-c0">"#)
            && m.contains(
                r#"<input type="checkbox" name="colour" value="g" checked id="colours-c1">"#
            )
            && m.contains(
                r#"<input type="checkbox" name="colour" value="b" checked id="colours-c2">"#
            ),
        "{m}"
    );
    assert!(
        m.contains(r#"type="submit" class="btn sm" data-noscript>Apply"#),
        "{m}"
    );
    // The enhanced control: a listbox that says it holds several, options that say which are chosen, a search field that drives it.
    assert!(
        m.contains(r#"role="listbox" aria-multiselectable="true" id="colours-list""#),
        "{m}"
    );
    assert!(
        m.contains(r#"id="colours-o1" data-i="1" data-value="g" data-label="Green" title="Green" aria-selected="true""#)
            && m.contains(r#"id="colours-o0" data-i="0" data-value="r" data-label="Red" title="Red" aria-selected="false""#),
        "{m}"
    );
    assert!(
        m.contains(r#"role="combobox" aria-autocomplete="list" aria-haspopup="listbox" aria-expanded="true" aria-controls="colours-list""#),
        "{m}"
    );
    assert!(
        m.contains(r#"x-data="dzMulti""#)
            && m.contains(r#" x-on:click.outside="away""#)
            && m.contains(r#" x-anchor.bottom-start.offset.4="$refs.trigger""#)
            && m.contains(r#"data-autosubmit"#),
        "{m}"
    );
    assert!(
        m.contains(r#"aria-controls="colours-pop""#) && m.contains(r#"id="colours-pop""#),
        "{m}"
    );
}

#[test]
fn the_button_says_the_selection_compactly() {
    assert_eq!(summary("All", "things", &[]), "All");
    assert_eq!(summary("All", "things", &["orders"]), "orders");
    assert_eq!(
        summary("All", "things", &["orders", "payments.settled"]),
        "orders, payments.settled"
    );
    assert_eq!(summary("All", "things", &["a", "b", "c"]), "3 things");
    let none = multi(&[]);
    assert!(
        none.contains(r#"<span class="dz-value muted">All colours</span>"#),
        "{none}"
    );
    assert!(!none.contains(" has\""), "{none}");
    assert!(multi(&["r"]).contains(r#"<span class="dz-value">Red</span>"#));
    assert!(multi(&["r", "g"]).contains(r#"<span class="dz-value">Red, Green</span>"#));
    let three = multi(&["r", "g", "b"]);
    assert!(
        three.contains(r#"<span class="dz-value">3 colours</span>"#),
        "{three}"
    );
    assert!(
        three.contains(r#"title="Red, Green, Blue""#) && three.contains("3 of 3 selected"),
        "{three}"
    );
    assert!(
        three.contains(r#"aria-label="Clear colours""#),
        "a clear x: {three}"
    );
}

#[test]
fn a_multiselect_lists_each_value_once_keeps_a_chosen_value_the_tape_lacks_and_shows_counts() {
    let cs = [
        Choice::new("a", "A").count(12),
        Choice::new("a", "A again").count(1),
        Choice::new("b", "B"),
    ];
    let values = vec!["gone".to_owned()];
    let m = multiselect(&MultiSelect::new("x", "x", "X", &cs, &values)).into_string();
    assert_eq!(
        m.matches(r#"role="option""#).count(),
        3,
        "a, b and the chosen one that is gone: {m}"
    );
    assert_eq!(m.matches(r#"name="x" value="a""#).count(), 1, "{m}");
    assert!(
        m.contains(r#"<span class="dz-count">12</span>"#) && !m.contains("A again"),
        "{m}"
    );
    assert!(
        m.contains(r#"value="gone" checked"#)
            && m.contains(
                r#"data-value="gone" data-label="gone" title="gone" aria-selected="true""#
            ),
        "{m}"
    );
}

#[test]
fn hostile_multiselect_labels_and_values_are_text() {
    let cs = [Choice::new(HOSTILE, HOSTILE).count(3)];
    let values = vec![HOSTILE.to_owned()];
    let m = multiselect(&MultiSelect::new("h", "h", HOSTILE, &cs, &values)).into_string();
    assert!(!m.contains("<img"), "{m}");
    assert!(
        !m.contains(r#"" onerror"#) && !m.contains("=alert(1)>"),
        "an attribute was injected: {m}"
    );
    assert!(
        m.contains("&lt;img src=x onerror=alert(1)&gt;") && m.contains("&quot;quoted&quot;"),
        "{m}"
    );
}

#[test]
fn the_multiselect_script_defines_its_component_and_every_method_the_markup_binds() {
    let js = include_str!("../../../assets/multiselect.js");
    assert!(js.contains("A.data('dzMulti'"));
    let m = multi(&[]);
    let mut bound = Vec::new();
    for attr in [
        "x-on:keydown=\"",
        "x-on:click=\"",
        "x-on:mousemove=\"",
        "x-on:mousedown=\"",
        "x-on:focusout=\"",
        "x-on:click.outside=\"",
        "x-on:input=\"",
    ] {
        let (_, rest) = m
            .split_once(attr)
            .unwrap_or_else(|| panic!("{attr} is not bound"));
        bound.push(rest.split('"').next().unwrap().to_owned());
    }
    for method in bound {
        assert!(
            js.contains(&format!("{method}(")),
            "multiselect.js has no method {method}"
        );
    }
}
