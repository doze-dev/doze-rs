//! Metric slots: how a page shows a fact that changes (CON-805, `docs/port/console/live-design.md`).
//!
//! A sentence is static text; a number that follows the broker is a [`Stat`]: a label, a value in a slot of reserved width
//! (tabular figures, `nowrap`, an ellipsis with the full text in `title`), an optional unit and an optional static tooltip. The
//! slot's width is declared by the page, never taken from the value, so a value of another length changes nothing around
//! it, and a strip is one line high whatever it says. Times are absolute; a relative form belongs in the tooltip, computed once.

use maud::{Markup, html};

/// One metric.
#[derive(Debug, Clone)]
pub struct Stat {
    label: String,
    value: String,
    unit: Option<String>,
    tip: Option<String>,
    ch: u8,
    mono: bool,
    time: Option<String>,
}

impl Stat {
    /// A metric with the default slot of 8 characters.
    #[must_use]
    pub fn new(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            unit: None,
            tip: None,
            ch: 8,
            mono: false,
            time: None,
        }
    }

    /// A unit after the value (`records`, `KiB`).
    #[must_use]
    pub fn unit(mut self, unit: impl Into<String>) -> Self {
        self.unit = Some(unit.into());
        self
    }

    /// A static tooltip: the exact value, or the relative form of a time. Computed at render; never updated.
    #[must_use]
    pub fn tip(mut self, tip: impl Into<String>) -> Self {
        self.tip = Some(tip.into());
        self
    }

    /// The slot's width in characters.
    #[must_use]
    pub const fn ch(mut self, ch: u8) -> Self {
        self.ch = ch;
        self
    }

    /// The value in the monospace face (a name, an address, a time).
    #[must_use]
    pub const fn mono(mut self) -> Self {
        self.mono = true;
        self
    }

    /// The value is an instant: `iso` goes in `datetime`; the hover shows the relative form, worked out when it is shown.
    #[must_use]
    pub fn time(mut self, iso: impl Into<String>) -> Self {
        self.time = Some(iso.into());
        self
    }

    /// An instant, written as `format::stamp` does: the text is absolute and does not change while the data does not.
    #[must_use]
    pub fn at(label: impl Into<String>, ms: i64, now_ms: i64) -> Self {
        Self::new(label, crate::format::stamp(ms, now_ms))
            .time(crate::format::iso(ms))
            .tip(crate::format::exact(ms))
            .mono()
            .ch(11)
    }

    fn one(&self, extra: &str) -> Markup {
        let style = format!("min-width:{}ch", self.ch);
        let class = if self.mono { "v mono" } else { "v" };
        let title = self.tip.as_deref().unwrap_or(&self.value);
        html! {
            div class={"stat" (extra)} {
                span class="k" { (self.label) }
                @if let Some(iso) = &self.time {
                    time class=(class) style=(style) datetime=(iso) title=(title) data-rel { (self.value) }
                } @else {
                    span class=(class) style=(style) title=(title) { (self.value) }
                }
                @if let Some(u) = &self.unit { span class="s" { (u) } }
            }
        }
    }
}

/// An instant in a table cell: absolute text, the exact time on hover (the script adds how long ago, when the pointer is on it).
#[must_use]
pub fn at_cell(ms: i64, now_ms: i64) -> Markup {
    html! { time class="mono" datetime=(crate::format::iso(ms)) title=(crate::format::exact(ms)) data-rel { (crate::format::stamp(ms, now_ms)) } }
}

/// A deadline `remaining_ms` from now: the absolute time, and a countdown the script keeps in this one element (CON-812).
#[must_use]
pub fn deadline(remaining_ms: i64) -> Markup {
    let at = (crate::source::now_ms() / 1000) * 1000 + remaining_ms.max(0) / 1000 * 1000;
    html! {
        span class="dl" {
            time class="mono" datetime=(crate::format::iso(at)) title=(crate::format::exact(at)) { (crate::format::clock(at)) " UTC" }
            " " span class="cd mono" data-until=(crate::format::iso(at)) {}
        }
    }
}

/// A value in a line of static text (a pager: `Showing [16] of [2,000] rows`): a slot of `ch` characters, so the line never changes length.
#[must_use]
pub fn slot(value: &str, ch: u8) -> Markup {
    html! { span class="slot" style={"width:" (ch) "ch"} title=(value) { (value) } }
}

/// A strip of metrics in one bordered row. It is one line high at any width the page is shown at, and never holds a sentence.
#[must_use]
pub fn stat_strip(stats: &[Stat]) -> Markup {
    html! { div class="stats" role="group" { @for s in stats { (s.one("")) } } }
}

/// A single metric as a tile.
#[must_use]
pub fn stat_tile(stat: &Stat) -> Markup {
    html! { div class="stats tile" role="group" { (stat.one("")) } }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stat_has_a_label_a_figure_of_declared_width_and_the_full_value_on_hover() {
        let m = stat_strip(&[Stat::new("Records", "1,644").unit("records").ch(9)]).into_string();
        assert_eq!(
            m,
            r#"<div class="stats" role="group"><div class="stat"><span class="k">Records</span><span class="v" style="min-width:9ch" title="1,644">1,644</span><span class="s">records</span></div></div>"#
        );
    }

    #[test]
    fn a_tip_replaces_the_value_on_hover_and_a_time_is_a_time_element() {
        let m = stat_tile(
            &Stat::new("Last write", "14:32:05")
                .time("2026-11-13T14:32:05Z")
                .tip("2026-11-13 14:32:05 UTC")
                .mono(),
        )
        .into_string();
        assert!(m.contains(r#"<time class="v mono" style="min-width:8ch" datetime="2026-11-13T14:32:05Z" title="2026-11-13 14:32:05 UTC" data-rel>14:32:05</time>"#), "{m}");
        assert!(m.starts_with(r#"<div class="stats tile""#));
    }

    #[test]
    fn an_instant_is_absolute_and_the_relative_form_is_not_in_the_text() {
        let m = stat_strip(&[Stat::at("Last write", 1_762_900_000_000, 1_762_900_005_000)])
            .into_string();
        assert!(m.contains(">22:26:40<") && !m.contains(" ago"), "{m}");
        let cell = at_cell(1_762_900_000_000, 1_762_900_005_000).into_string();
        assert!(
            cell.contains("data-rel") && cell.contains("2025-11-11 22:26:40 UTC"),
            "{cell}"
        );
    }

    #[test]
    fn text_is_escaped() {
        let m = stat_strip(&[Stat::new("<l>", "<v>")]).into_string();
        assert!(!m.contains("<l>") && !m.contains("<v>"));
    }
}
