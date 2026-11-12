//! The Flags section: every flag the broker was started with, its value, its default and what it does; the ones that differ
//! from their default first, all of them behind "Show all".

use maud::{Markup, html};

use crate::base::Base;
use crate::model::SettingsFlag;
use crate::ui::glyph::{Glyph, glyph};
use crate::ui::glyph_extra::{Extra, glyph_extra};

fn colgroup() -> Markup {
    html! { colgroup { col style="width:27%"; col style="width:20%"; col style="width:17%"; col; } }
}

fn pills(f: &SettingsFlag) -> Markup {
    html! {
        @if f.differs() { " " span class="pill" { "set" } }
        @if f.dynamic.is_some() { " " a class="pill" href="#broker-configs" { "also dynamic" } }
    }
}

fn row(f: &SettingsFlag) -> Markup {
    html! {
        tr {
            td class="mono name" { (f.name) }
            td class="mono" { (f.value) }
            td class="mono dim" { (f.default) }
            td { (f.what) (pills(f)) }
        }
    }
}

fn head() -> Markup {
    html! { thead { tr { th scope="col" { "Flag" } th scope="col" { "Value" } th scope="col" { "Default" } th scope="col" { "What it does" } } } }
}

fn grouped(flags: &[SettingsFlag]) -> Markup {
    let mut groups: Vec<&str> = Vec::new();
    for f in flags {
        if !groups.contains(&f.group.as_str()) {
            groups.push(&f.group);
        }
    }
    html! {
        table class="t" style="table-layout:fixed;margin-top:12px" aria-label="Every flag, by group" {
            (colgroup())
            tbody {
                @for g in groups {
                    tr { td colspan="4" style="background:var(--well);height:44px" { b { (g) } } }
                    @for f in flags.iter().filter(|f| f.group == g) { (row(f)) }
                }
            }
        }
    }
}

fn differing(flags: &[SettingsFlag]) -> Markup {
    let diff: Vec<&SettingsFlag> = flags.iter().filter(|f| f.differs()).collect();
    html! {
        table class="t" style="table-layout:fixed" {
            (colgroup())
            (head())
            tbody {
                @if diff.is_empty() {
                    tr { td colspan="4" class="dim" { "Every flag is as shipped." } }
                }
                @for f in diff { (row(f)) }
            }
        }
    }
}

fn lede(flags: &[SettingsFlag]) -> Markup {
    let diff: Vec<&SettingsFlag> = flags.iter().filter(|f| f.differs()).collect();
    html! {
        p class="lede" {
            "What the broker was started with, each with its default. " b { "Read only: set at start." } " "
            @if diff.is_empty() {
                b { "No" } " flag differs from its default."
            } @else {
                b { (diff.len()) } " flag" @if diff.len() != 1 { "s" } " differ" @if diff.len() == 1 { "s" } " from " @if diff.len() == 1 { "its" } @else { "their" } " default: "
                @for (i, f) in diff.iter().enumerate() {
                    @if i > 0 { ", " }
                    span class="mono" { (f.name) }
                }
                ". The rest are as shipped."
            }
        }
    }
}

/// The section.
#[must_use]
pub(super) fn flags_section(flags: &[SettingsFlag], base: &Base) -> Markup {
    html! {
        div class="sec" id="flags" {
            h2 { "Flags" }
            @if flags.is_empty() {
                p class="lede" { "What the broker was started with, each with its default. " b { "Read only: set at start." } }
                p class="help" { "This broker did not say how it was started, so there is no table of flags to show." }
            } @else {
                (lede(flags))
                (differing(flags))
                details style="margin-top:12px" {
                    summary class="btn sm" style="list-style:none;cursor:pointer" { (glyph_extra(Extra::ChevD, 14)) "Show all " (flags.len()) " flags" }
                    (grouped(flags))
                }
            }
            p class="help" style="margin:20px 0 10px" {
                b { "Refused at start, with exit code 2." }
                " A broker that ignored a security flag would be open while looking closed, so it will not start with SASL and nobody who could log in, with a certificate it cannot read, or with the console beyond loopback while SASL or TLS is on (unless "
                span class="mono" { "--console-unprotected" } " accepts it)."
            }
            div class="row wrap" style="gap:10px" {
                span class="tag" { (glyph(Glyph::Lock, 12, "")) "Exit 2" }
                a class="lnk" href=(base.url("/access")) { "What is on, in Access" }
            }
        }
    }
}
