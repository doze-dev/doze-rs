//! `/gallery` (the examples build only): every component and state in one place, for the owner's eyes and for the interaction
//! tests. Nothing here is a page of the console; the `boards` example serves it, a release build has no such route.

use maud::{Markup, html};

use super::{
    Choice, Combobox, Menu, MenuItem, MultiSelect, SegItem, Select, combobox, menu, multiselect,
    popover, seg, select, tip,
};
use crate::ui::client::{self, in_place_form};
use crate::ui::layout::page_header;

/// A label that tries to break out of its markup: it must show as text everywhere.
pub const HOSTILE: &str = r#"<img src=x onerror=alert(1)> "quoted" & 'single'"#;

/// The id of the region the demo's controls change.
pub const REGION: &str = "gal-data";

/// What the demo's controls chose, from the query of the address.
#[derive(Debug, Default, Clone)]
pub struct Chosen {
    pub partition: String,
    pub topic: String,
    pub client: String,
    pub view: String,
    /// The topics the multi-select chose (the repeated `tag` field).
    pub tags: Vec<String>,
}

impl Chosen {
    /// Reads `partition`, `topic`, `client` and `view` from a query string.
    #[must_use]
    pub fn from_query(query: &str) -> Self {
        let f = crate::form::parse(query);
        let get = |k| crate::form::get(&f, k).unwrap_or("").to_owned();
        Self {
            partition: get("partition"),
            topic: get("topic"),
            client: get("client"),
            view: get("view"),
            tags: f
                .iter()
                .filter(|(k, v)| k == "tag" && !v.is_empty())
                .map(|(_, v)| v.clone())
                .collect(),
        }
    }
}

fn section(title: &str, note: &str, body: Markup) -> Markup {
    html! { section { h2 { (title) } p class="help" style="margin:0" { (note) } div class="row" { (body) } } }
}

fn demo_form(c: &Chosen, base: &crate::base::Base) -> Markup {
    let partitions = [
        Choice::new("", "All partitions"),
        Choice::new("0", "0"),
        Choice::new("1", "1").desc("leader 2"),
        Choice::new("2", "2"),
    ];
    let topics: Vec<String> = (1..=40).map(|n| format!("topic.{n:02}")).collect();
    let mut tc = vec![Choice::new("", "Any topic")];
    tc.extend(topics.iter().map(|t| Choice::new(t, t)));
    let tag_choices: Vec<Choice<'_>> = topics
        .iter()
        .enumerate()
        .map(|(i, t)| Choice::new(t, t).count(i * 7 % 23 + 1))
        .collect();
    let seg_items: Vec<(String, String)> = ["all", "ok", "refused"]
        .iter()
        .map(|v| {
            (
                (*v).to_owned(),
                format!(
                    "{}?view={v}&partition={}&topic={}",
                    base.url("/gallery"),
                    c.partition,
                    c.topic
                ),
            )
        })
        .collect();
    let items: Vec<SegItem<'_>> = seg_items
        .iter()
        .map(|(v, href)| SegItem {
            label: v,
            href,
            current: c.view == *v || (c.view.is_empty() && v == "all"),
            count: None,
        })
        .collect();
    html! {
        (in_place_form(REGION, "toolbar", &base.url("/gallery"), html! {
            (select(&Select { auto_submit: true, placeholder: "All partitions", ..Select::new("g-partition", "partition", "Partition", &partitions, &c.partition) }))
            (combobox(&Combobox { auto_submit: true, placeholder: "Search topics", ..Combobox::new("g-topic", "topic", "Topic", &tc, &c.topic) }))
            (multiselect(&MultiSelect { auto_submit: true, placeholder: "All topics", noun: "topics", class: "dz-chip", ..MultiSelect::new("g-tags", "tag", "Topics", &tag_choices, &c.tags) }))
            input type="hidden" name="view" value=(c.view);
            noscript { button class="btn" type="submit" { "Filter" } }
        }))
        div style="margin-top:12px" { (seg("View", &items, Some(REGION))) }
    }
}

/// The region the controls change: it says what they chose, and is tall enough to scroll.
#[must_use]
pub fn region(c: &Chosen) -> Markup {
    html! {
        div class="frag" id=(REGION) {
            p class="sub" data-test="chosen" { "partition " b { (if c.partition.is_empty() { "all" } else { &c.partition }) } ", topic " b { (if c.topic.is_empty() { "any" } else { &c.topic }) } ", topics " b { (if c.tags.is_empty() { "all".to_owned() } else { c.tags.join(" + ") }) } ", view " b { (if c.view.is_empty() { "all" } else { &c.view }) } }
            div style="height:520px;background:linear-gradient(var(--well),#fff);border:1px dashed var(--line2);border-radius:12px;padding:16px" { "A tall region, so the page scrolls: a control above must not move it." }
        }
    }
}

fn colours_and_clients() -> (Vec<Choice<'static>>, Vec<Choice<'static>>) {
    let colours = vec![
        Choice::new("r", "Red"),
        Choice::new("g", "Green").desc("the second one"),
        Choice::new("b", HOSTILE),
        Choice::new("n", "Not on offer").disabled(),
    ];
    let clients = [
        "admin-cli",
        "search-indexer",
        "checkout-svc",
        "billing-batch",
    ];
    (colours, clients.iter().map(|n| Choice::new(n, n)).collect())
}

fn choice_sections() -> Markup {
    let (colours, cc) = colours_and_clients();
    html! {
        (section("Select", "Single choice. Keyboard: Enter, Space or the arrows open; arrows, Home, End and letters move; Enter chooses; Esc closes.", html! {
            (select(&Select::new("s-plain", "colour", "Colour", &colours, "g")))
            (select(&Select { placeholder: "Pick a colour", ..Select::new("s-empty", "colour2", "Colour", &colours, "") }))
            (select(&Select { disabled: true, ..Select::new("s-dis", "colour3", "Colour", &colours, "r") }))
            (select(&Select { class: "dz-chip", ..Select::new("s-chip", "client", "Client", &cc, "") }))
        }))
        (section("MultiSelect", "Several choices: a button that says them compactly, a clear x, a popup with a search field, Select all and Clear, and checks. Arrows move; Space or Enter toggles; Backspace in the empty search removes the last; Esc closes.", html! {
            (multiselect(&MultiSelect { placeholder: "All clients", noun: "clients", ..MultiSelect::new("m-none", "mclient", "Clients", &cc, &[]) }))
            (multiselect(&MultiSelect { placeholder: "All clients", noun: "clients", class: "dz-chip", ..MultiSelect::new("m-one", "mclient1", "Clients", &cc, &["admin-cli".to_owned()]) }))
            (multiselect(&MultiSelect { placeholder: "All clients", noun: "clients", class: "dz-chip", ..MultiSelect::new("m-three", "mclient3", "Clients", &cc, &["admin-cli".to_owned(), "checkout-svc".to_owned(), "billing-batch".to_owned()]) }))
            (multiselect(&MultiSelect { placeholder: "All colours", noun: "colours", ..MultiSelect::new("m-hostile", "mcolour", "Colours", &colours, &[HOSTILE.to_owned()]) }))
        }))
        (section("Combobox", "Searchable. Type to filter; with free text it offers to use what you typed.", html! {
            (combobox(&Combobox::new("c-client", "cclient", "Client", &cc, "")))
            (combobox(&Combobox { create: true, placeholder: "Any client id", ..Combobox::new("c-free", "cfree", "Client id", &cc, "") }))
        }))
    }
}

fn floating_sections() -> Markup {
    let items = [
        MenuItem::Link {
            label: "Open the topic",
            href: "/topics/orders",
            dialog: false,
            danger: false,
        },
        MenuItem::Link {
            label: "Produce a record",
            href: "/topics/orders/produce",
            dialog: true,
            danger: false,
        },
        MenuItem::Separator,
        MenuItem::Link {
            label: "Delete topic",
            href: "/topics/orders/delete",
            dialog: true,
            danger: true,
        },
    ];
    let rows = [
        SegItem {
            label: "All partitions",
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
        SegItem {
            label: "1",
            href: "?p=1",
            current: false,
            count: None,
        },
        SegItem {
            label: "2",
            href: "?p=2",
            current: false,
            count: None,
        },
    ];
    html! {
        (section("Dropdown menu", "A \"...\" button, as in the last cell of a row: the menu opens under it, its right edge on the button's. Arrows move, Esc returns to the button.", html! {
            div style="margin-left:200px" { (menu(&Menu { id: "m1", label: "Actions for orders", items: &items })) }
        }))
        (section("Tooltip and popover", "A tooltip is 400 ms of hover or focus; a popover opens on click.", html! {
            (tip("t1", "Copy the address", &html! { button class="btn sm" type="button" { "Hover me" } }))
            (tip("t2", HOSTILE, &html! { span class="mono" tabindex="0" { "a truncated value…" } }))
            (popover("p1", "Details", &html! { p style="margin:0 0 6px" { b { "orders-0" } } p class="help" style="margin:0" { "A small floating card with its own text." } }))
        }))
        (section("Segmented control", "A row of links, one stop in the Tab order; arrows move focus.", seg("Partitions", &rows, None)))
    }
}

/// The gallery page's `main`.
#[must_use]
pub fn page(c: &Chosen, base: &crate::base::Base) -> Markup {
    html! {
        (page_header("Component gallery", html! { p class="sub" { "Every component and state. Examples build only." } }, html! {}))
        div class="gal" {
            (section("In place", "These controls change only the region below and push the address; scroll, focus and typed text stay.", demo_form(c, base)))
            (region(c))
            (choice_sections())
            (floating_sections())
            (section("Overlays", "A drawer and a dialog.", html! {
                (client::dialog_link("btn", &base.url("/topics/orders/produce"), html! { "Open a dialog" }))
                (client::dialog_link("btn", &base.url("/topics/orders/messages/1/16050"), html! { "Open a drawer" }))
            }))
        }
    }
}
