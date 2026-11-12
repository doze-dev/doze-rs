//! The page: title, the six section tabs, the sentence under them, and the six sections.

use maud::{Markup, html};

use super::configs::configs_section;
use super::data::data_section;
use super::flags::flags_section;
use super::intervals::intervals_section;
use super::profile::profile_section;
use super::weight::weight_section;
use crate::base::Base;
use crate::model::{SettingsHead, SettingsView};
use crate::ui::layout::page_header;

const TABS: [(&str, &str); 6] = [
    ("broker-configs", "Broker configs"),
    ("version-profile", "Version profile"),
    ("intervals", "Intervals"),
    ("weight", "Weight"),
    ("data", "Data"),
    ("flags", "Flags"),
];

/// The tabs: each is a link to its section of this page.
pub(super) fn tabs() -> Markup {
    html! {
        nav class="tabs" aria-label="Sections of Settings" {
            @for (i, (id, label)) in TABS.iter().enumerate() {
                a class={"tab" @if i == 0 { " on" }} href={"#" (id)} { (label) }
            }
        }
    }
}

/// The sentence under the title.
pub(super) fn sub() -> Markup {
    html! {
        p class="sub" {
            "This broker's own configuration, explained. " b { "Five settings can change while it runs" }
            "; everything else was fixed when it started and is shown read only."
        }
    }
}

fn sentence(h: &SettingsHead) -> Markup {
    html! {
        p class="sent" data-extra="live-sentence" style="margin:-8px 0 28px" {
            @if h.set == 0 {
                b { "0" } " of the 5 are set at run time: all five are at the values the broker was started with."
            } @else {
                b { (h.set) } " of the 5 " @if h.set == 1 { "is" } @else { "are" } " set at run time."
            }
            @if let (Some(up), Some(since)) = (&h.up, &h.since) {
                " Running " b { (up) } " since " (since) ", version profile " b { (h.profile) } "."
            } @else {
                " Version profile " b { (h.profile) } "."
            }
        }
    }
}

/// The Settings page.
#[must_use]
pub fn settings_page(v: &SettingsView, base: &Base) -> Markup {
    html! {
        (page_header("Settings", sub(), html! {}))
        (tabs())
        (sentence(&v.head))
        (configs_section(&v.configs, base))
        (profile_section(&v.profile, v.head.profile.as_str()))
        (intervals_section(&v.intervals))
        (weight_section(&v.weight))
        (data_section(&v.data, &v.intervals))
        (flags_section(&v.flags, base))
    }
}
