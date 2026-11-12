//! The Weight section: what this instance costs the machine, from what the process can read of itself, with the limits the
//! project holds itself to (`docs/port/weight.md`, "The CI limits").

use maud::{Markup, html};

use crate::format::{bytes, number};
use crate::model::SettingsWeight;
use crate::settings_words::{LIMIT_BINARY_BYTES, LIMIT_IDLE_KB};

/// `4.5` and `MB`, as the big figure is written: one decimal, decimal megabytes as the limits are.
fn mb(n: u64) -> (String, &'static str) {
    #[allow(clippy::cast_precision_loss)]
    let v = n as f64 / 1_000_000.0;
    (format!("{v:.1}"), "MB")
}

fn tile(label: &str, big: Markup, under: Markup) -> Markup {
    html! {
        div class="card" style="padding:24px" {
            div class="eyebrow" { (label) }
            div class="big" style="margin-top:8px" { (big) }
            (under)
        }
    }
}

fn memory(w: &SettingsWeight) -> Markup {
    match w.resident {
        Some(r) => {
            let (n, u) = mb(r);
            let limit = format!("limit {} MB", LIMIT_IDLE_KB / 1024);
            tile(
                "Idle memory",
                html! { (n) small { (u) } },
                html! { p class="help" style="margin:10px 0 0" {
                    "resident" @if let Some(t) = w.threads { ", " (t) " threads" } "; " (limit)
                } },
            )
        }
        None => tile(
            "Idle memory",
            html! { "n/a" },
            html! { p class="help" style="margin:10px 0 0" { "Measured on Linux only: this system does not let a process read its own resident memory without unsafe code" } },
        ),
    }
}

fn partitions(w: &SettingsWeight) -> Markup {
    let p = (w.open_partitions * 100)
        .checked_div(w.max_open)
        .unwrap_or(0)
        .min(100);
    tile(
        "Open partitions",
        html! { (w.open_partitions) small { @if w.max_open > 0 { "of " (w.max_open) } @else { "no limit" } } },
        html! { @if w.max_open > 0 { div class="lagbar" style="margin-top:12px" { div class="tr" { i style={"--p:" (p) "%"} {} } } } },
    )
}

fn descriptors(w: &SettingsWeight) -> Markup {
    match w.open_files {
        Some(n) => tile(
            "File descriptors",
            html! { (n) small { "open" } },
            html! { p class="help" style="margin:10px 0 0" { "in this process: open partitions, their segments and indexes, and sockets" } },
        ),
        None => tile(
            "File descriptors",
            html! { "n/a" },
            html! { p class="help" style="margin:10px 0 0" { "this system does not list a process's open files" } },
        ),
    }
}

fn binary(w: &SettingsWeight) -> Markup {
    match w.binary {
        Some(b) => {
            let (n, u) = mb(b);
            let (limit, _) = mb(LIMIT_BINARY_BYTES);
            tile(
                "Binary",
                html! { (n) small { (u) } },
                html! { p class="help" style="margin:10px 0 0" { "the broker program, with the console in it; the limit is " (limit) " MB" } },
            )
        }
        None => tile(
            "Binary",
            html! { "n/a" },
            html! { p class="help" style="margin:10px 0 0" { "the program's own file could not be read" } },
        ),
    }
}

fn allowed(w: &SettingsWeight) -> Markup {
    html! {
        dl class="kv sm" style="margin-top:24px" {
            dt { "Allowed open" }
            dd {
                @if w.max_open > 0 { (w.max_open) } @else { "no limit" }
                span class="sub" {
                    @match &w.flag_max_open {
                        Some(f) if f.parse::<usize>().is_ok_and(|f| w.max_open > 0 && f != w.max_open) => {
                            "--max-open-partitions says " (f) "; the limit on file descriptors this process has leaves room for only " (w.max_open) ", at 4 each"
                        }
                        Some(f) => { "--max-open-partitions " (f) ", within what this process's limit on file descriptors leaves, at 4 each" }
                        None => { "the lower of --max-open-partitions and what this process's limit on file descriptors leaves, at 4 each" }
                    }
                }
            }
        }
    }
}

/// The section.
#[must_use]
pub(super) fn weight_section(w: &SettingsWeight) -> Markup {
    html! {
        div class="sec" id="weight" {
            h2 { "Weight" }
            p class="lede" {
                "What this instance costs your machine. Weight is a feature: idle memory is measured in the tests and may only rise with a written reason. Right now "
                b { (w.open_partitions) } " partitions are open"
                @if w.max_open > 0 { " of the " b { (w.max_open) } " it will keep open" }
                @if let Some(n) = w.open_files { ", with " b { (number(i64::try_from(n).unwrap_or(0))) } " file descriptors open in the process" }
                "."
            }
            div class="g4" { (memory(w)) (partitions(w)) (descriptors(w)) (binary(w)) }
            (allowed(w))
            p class="help" style="margin:8px 0 0" {
                "Partitions open when asked for" @if w.partitions > 0 { " (" (w.partitions) " in " (w.topics) " topics)" } ", and the least recently used closes, after a flush, once the limit is reached. The cleaner holds nothing while idle: a round builds a key map of at most 16 MiB and drops it when the round ends. Big requests reuse buffers from a pool of at most 64 MiB, so memory after a heavy produce stays near what it was."
                @if let Some(b) = w.binary { " The binary is " (bytes(b)) "." }
            }
        }
    }
}
