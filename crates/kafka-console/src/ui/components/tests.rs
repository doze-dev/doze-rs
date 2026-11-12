use maud::html;

use super::gallery::HOSTILE;
use super::*;

const COLOURS: [Choice<'static>; 3] = [
    Choice::new("r", "Red"),
    Choice::new("g", "Green"),
    Choice::new("b", "Blue"),
];

fn sel(value: &str) -> String {
    select(&Select::new("colour", "colour", "Colour", &COLOURS, value)).into_string()
}

// ---- Select ----------------------------------------------------------------------------------------------------------

#[test]
fn a_select_is_a_real_select_underneath_and_a_listbox_on_top() {
    let m = sel("g");
    // The no-script control, with its id, name, label and the chosen option.
    assert!(
        m.contains(r#"<select class="dz-native" id="colour" name="colour" aria-label="Colour">"#),
        "{m}"
    );
    assert!(
        m.contains(r#"<option value="g" selected>Green</option>"#),
        "{m}"
    );
    // The enhanced control: a combobox button that says what it controls, a listbox, options with ids.
    assert!(
        m.contains(r#"role="combobox" aria-haspopup="listbox" aria-expanded="false""#),
        "{m}"
    );
    assert!(
        m.contains(r#"aria-controls="colour-list""#)
            && m.contains(r#"role="listbox" id="colour-list""#),
        "{m}"
    );
    assert!(
        m.contains(
            r#"id="colour-o1" data-value="g" data-label="Green" title="Green" aria-selected="true""#
        ),
        "{m}"
    );
    assert!(
        m.contains(
            r#"id="colour-o0" data-value="r" data-label="Red" title="Red" aria-selected="false""#
        ),
        "{m}"
    );
    // Alpine's attributes, spelled the way Alpine reads them.
    assert!(
        m.contains(r#"x-data="dzSelect""#) && m.contains(r#" x-on:click.outside="away""#),
        "{m}"
    );
    assert!(
        m.contains(r#" x-anchor.bottom-start.offset.4="$refs.trigger""#)
            && m.contains(r#" :aria-expanded="open""#),
        "{m}"
    );
    // Out of sight without Alpine: the button is hidden and the list has no display.
    assert!(
        m.contains(" hidden>") && m.contains(r#"style="display:none""#),
        "{m}"
    );
}

#[test]
fn a_select_with_nothing_chosen_shows_the_placeholder_and_offers_no_first_option_by_accident() {
    let m = select(&Select {
        placeholder: "Pick one",
        ..Select::new("c", "c", "Colour", &COLOURS, "")
    })
    .into_string();
    assert!(
        m.contains(r#"<option value="" selected hidden>Pick one</option>"#),
        "{m}"
    );
    assert!(
        m.contains(r#"<span class="dz-value muted">Pick one</span>"#),
        "{m}"
    );
    // A choice that is the empty value is the "all" choice: no placeholder option is added.
    let all = [Choice::new("", "All"), Choice::new("a", "A")];
    let m = select(&Select::new("c", "c", "x", &all, "")).into_string();
    assert!(!m.contains(r#"<option value="" selected hidden>"#), "{m}");
    assert!(m.contains(r#"<span class="dz-value">All</span>"#), "{m}");
}

#[test]
fn a_value_no_choice_has_stays_a_choice_so_the_page_never_lies() {
    let m = sel("zzz");
    assert!(
        m.contains(r#"<option value="zzz" selected>zzz</option>"#),
        "{m}"
    );
}

#[test]
fn hostile_labels_values_and_descriptions_are_text_everywhere() {
    let cs = [Choice::new(HOSTILE, HOSTILE).desc(HOSTILE)];
    for m in [
        select(&Select::new("s", "s", HOSTILE, &cs, HOSTILE)).into_string(),
        combobox(&Combobox::new("c", "c", HOSTILE, &cs, HOSTILE)).into_string(),
    ] {
        assert!(!m.contains("<img"), "{m}");
        assert!(
            !m.contains(r#"" onerror"#) && !m.contains("=alert(1)>"),
            "an attribute was injected: {m}"
        );
        assert!(m.contains("&lt;img src=x onerror=alert(1)&gt;"), "{m}");
        assert!(m.contains("&quot;quoted&quot;"), "{m}");
    }
}

#[test]
fn a_select_can_submit_its_form_and_be_disabled() {
    let m = select(&Select {
        auto_submit: true,
        disabled: true,
        class: "dz-chip",
        ..Select::new("a", "a", "A", &COLOURS, "r")
    })
    .into_string();
    assert!(
        m.contains(r#"data-autosubmit"#)
            && m.contains("disabled")
            && m.contains(r#"class="dz dz-select dz-chip""#),
        "{m}"
    );
    let plain = sel("r");
    assert!(
        !plain.contains("data-autosubmit") && !plain.contains("disabled"),
        "{plain}"
    );
}

#[test]
fn a_disabled_choice_is_marked_for_both_controls() {
    let cs = [Choice::new("a", "A"), Choice::new("b", "B").disabled()];
    let m = select(&Select::new("d", "d", "D", &cs, "a")).into_string();
    assert!(
        m.contains(r#"<option value="b" disabled>B</option>"#)
            && m.contains(r#"aria-disabled="true""#),
        "{m}"
    );
}

// ---- Combobox --------------------------------------------------------------------------------------------------------

#[test]
fn a_combobox_is_a_select_underneath_and_a_searchable_listbox_on_top() {
    let m = combobox(&Combobox::new("topic", "topic", "Topic", &COLOURS, "b")).into_string();
    assert!(
        m.contains(r#"<select class="dz-native" id="topic" name="topic" aria-label="Topic">"#),
        "{m}"
    );
    assert!(m.contains(r#"role="combobox" aria-autocomplete="list" aria-haspopup="listbox" aria-expanded="false""#), "{m}");
    assert!(
        m.contains(r#"value="Blue""#),
        "the field starts with the chosen label: {m}"
    );
    assert!(
        m.contains(r#"class="dz-empty" hidden>No match</div>"#),
        "{m}"
    );
    assert!(
        m.contains(r#"data-create-row"#) && m.contains(r#"x-data="dzCombobox""#),
        "{m}"
    );
    assert!(
        !m.contains(" data-create "),
        "a closed list does not offer to create: {m}"
    );
}

#[test]
fn a_combobox_for_free_text_is_a_text_field_with_a_datalist_underneath() {
    let m = combobox(&Combobox {
        create: true,
        ..Combobox::new("c", "client", "Client", &COLOURS, "mine")
    })
    .into_string();
    assert!(
        m.contains(
            r#"<input class="dz-native" type="text" id="c" name="client" value="mine" list="c-dl""#
        ),
        "{m}"
    );
    assert!(
        m.contains(r#"<datalist id="c-dl"><option value="r">Red</option>"#),
        "{m}"
    );
    assert!(m.contains("data-create"), "{m}");
    assert!(
        m.contains(r#"value="mine" x-on:input="input""#),
        "the visible field starts with the free text: {m}"
    );
}

// ---- Menu ------------------------------------------------------------------------------------------------------------

#[test]
fn a_menu_is_a_button_and_a_menu_whose_items_are_links_and_buttons() {
    let items = [
        MenuItem::Link {
            label: "Open",
            href: "/t/orders",
            dialog: false,
            danger: false,
        },
        MenuItem::Separator,
        MenuItem::Link {
            label: "Delete",
            href: "/t/orders/delete",
            dialog: true,
            danger: true,
        },
        MenuItem::Button {
            label: "Reset",
            form: "f",
            name: "act",
            value: "reset",
            danger: false,
        },
    ];
    let m = menu(&Menu {
        id: "m",
        label: "Actions for orders",
        items: &items,
    })
    .into_string();
    assert!(m.contains(r#"aria-haspopup="menu" aria-expanded="false" :aria-expanded="open" aria-controls="m-menu" aria-label="Actions for orders" hidden"#), "{m}");
    assert!(
        m.contains(r#"role="menu" id="m-menu" aria-label="Actions for orders""#),
        "{m}"
    );
    assert!(
        m.contains(r#"<a class="dz-item" role="menuitem" id="m-i0" href="/t/orders">Open</a>"#),
        "{m}"
    );
    assert!(
        m.contains(r#"<div class="dz-sep" role="separator"></div>"#),
        "{m}"
    );
    assert!(m.contains(r#"<a class="dz-item danger" role="menuitem" id="m-i2" href="/t/orders/delete" data-dialog>Delete</a>"#), "{m}");
    assert!(m.contains(r#"<button class="dz-item" role="menuitem" id="m-i3" type="submit" form="f" name="act" value="reset">Reset</button>"#), "{m}");
}

#[test]
fn a_hostile_menu_label_is_text() {
    let items = [MenuItem::Link {
        label: HOSTILE,
        href: "/x",
        dialog: false,
        danger: false,
    }];
    let m = menu(&Menu {
        id: "m",
        label: HOSTILE,
        items: &items,
    })
    .into_string();
    assert!(!m.contains("<img"), "{m}");
}

// ---- Tooltip, popover, segmented control -----------------------------------------------------------------------------

#[test]
fn a_tooltip_is_the_title_without_a_script_and_a_described_tooltip_with_one() {
    let m = tip("t1", "Copy it", &html! { button { "Copy" } }).into_string();
    assert!(
        m.contains(r#"title="Copy it" aria-describedby="t1""#),
        "{m}"
    );
    assert!(
        m.contains(r#"role="tooltip" id="t1""#)
            && m.contains(r#"x-show="shown""#)
            && m.contains(" x-anchor.top.offset.6="),
        "{m}"
    );
    assert!(m.contains("<button>Copy</button>"), "{m}");
    let h = tip("t2", HOSTILE, &html! { "x" }).into_string();
    assert!(
        !h.contains("<img") && !h.contains("onerror=alert(1)>"),
        "{h}"
    );
}

#[test]
fn a_popover_is_a_button_and_a_dialog_card_that_shows_inline_without_a_script() {
    let m = popover("p", "Details", &html! { p { "body" } }).into_string();
    assert!(m.contains(r#"aria-haspopup="dialog" aria-expanded="false" :aria-expanded="open" aria-controls="p" id="p-btn" hidden"#), "{m}");
    assert!(
        m.contains(r#"role="dialog" id="p" aria-labelledby="p-btn""#) && m.contains("<p>body</p>"),
        "{m}"
    );
}

#[test]
fn a_segmented_control_is_a_group_of_links_with_one_current_and_may_work_in_place() {
    let items = [
        SegItem {
            label: "All",
            href: "?p=all",
            current: true,
            count: None,
        },
        SegItem {
            label: "0",
            href: "?p=0",
            current: false,
            count: None,
        },
    ];
    let plain = seg("Partitions", &items, None).into_string();
    assert!(
        plain.contains(r#"role="group" aria-label="Partitions""#)
            && plain.contains(r#"aria-current="true""#),
        "{plain}"
    );
    assert_eq!(plain.matches("aria-current").count(), 1);
    assert!(
        !plain.contains("hx-get"),
        "a plain link needs no htmx: {plain}"
    );
    let live = seg("Partitions", &items, Some("msgs")).into_string();
    assert!(live.contains(r##"hx-get="?p=0" hx-target="#msgs" hx-select="#msgs" hx-swap="morph:outerHTML show:none" hx-push-url="true""##), "{live}");
    assert!(
        live.contains(r#"href="?p=0""#),
        "it is still a link: {live}"
    );
}

#[test]
fn the_script_defines_every_component_the_markup_names() {
    let js = include_str!("../../../assets/components.js");
    for name in [
        "dzSelect",
        "dzCombobox",
        "dzMenu",
        "dzTip",
        "dzPopover",
        "dzSeg",
    ] {
        assert!(
            js.contains(&format!("A.data('{name}'")),
            "components.js does not define {name}"
        );
    }
    // Every method the markup binds exists in the script.
    for method in [
        "key", "click", "hover", "focusout", "away", "later", "now", "off", "toggle", "focus",
        "input",
    ] {
        assert!(
            js.contains(&format!("{method}(")),
            "components.js has no method {method}"
        );
    }
}

#[test]
fn the_alpine_stand_ins_never_leak_into_the_page() {
    let all = [
        sel("r"),
        combobox(&Combobox::new("a", "a", "A", &COLOURS, "")).into_string(),
        tip("t", "x", &html! {}).into_string(),
        popover("p", "x", &html! {}).into_string(),
        menu(&Menu {
            id: "m",
            label: "x",
            items: &[],
        })
        .into_string(),
    ]
    .join("");
    let all = format!("{all}{}", super::tests_multi::multi(&["r"]));
    for stand_in in [" xo-click=", " xa-", " xb-"] {
        assert!(!all.contains(stand_in), "{stand_in} is in the markup");
    }
}
