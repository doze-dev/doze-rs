//! The Refresh control (CON-802): at the right of the breadcrumb row of a data page. A button (key `r`), the time the data was
//! rendered (absolute, UTC, never "s ago") and an interval picker (Off, 5 s, 10 s, 30 s, 1 min; default Off). With a script it
//! fetches the page's regions again, with their ETags, now or on the interval; without one the button is a link that reloads the
//! page and the picker is not offered. The behaviour is `assets/live.js` (`refreshControl`).

use maud::{Markup, html};

use super::components::{Choice, Select, select};
use super::glyph::{Glyph, glyph};
use crate::format::{civil, clock};

/// The intervals offered, in seconds, with their labels. 0 is Off. The least is 5 s, the same floor Grafana enforces.
pub const INTERVALS: [(&str, &str); 5] = [
    ("0", "Off"),
    ("5", "5 s"),
    ("10", "10 s"),
    ("30", "30 s"),
    ("60", "1 min"),
];

/// The control for a page whose regions are `regions`, rendered at `now_ms`.
#[must_use]
pub fn refresh_bar(regions: &[&str], now_ms: i64) -> Markup {
    let c = civil(now_ms);
    let iso = format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        c.year, c.month, c.day, c.hour, c.minute, c.second
    );
    let choices: Vec<Choice<'_>> = INTERVALS.iter().map(|(v, l)| Choice::new(v, l)).collect();
    html! {
        div class="refresh" x-data="refreshControl" x-on:change="picked" data-refresh data-regions=(regions.join(",")) {
            span class="stamp" { "Updated " time data-stamp data-vary datetime=(iso) title="UTC" { (clock(now_ms)) } }
            span class="every" {
                (select(&Select { class: "dz-chip", ui: Some("refresh-every"), ..Select::new("refresh-every", "every", "Refresh every", &choices, "0") }))
            }
            span class="bad" data-refresh-failed hidden {}
            a class="btn sm" href="" data-refresh-now title="Refresh (r)" aria-label="Refresh" x-on:click="now" { (glyph(Glyph::Refresh, 14, "")) "Refresh" }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_control_names_its_regions_stamps_the_render_time_and_defaults_to_off() {
        let m = refresh_bar(&["topics-table"], 1_762_900_000_000).into_string();
        assert!(m.contains(r#"data-regions="topics-table""#));
        assert!(m.contains(r#"datetime="2025-11-11T22:26:40Z""#) && m.contains(">22:26:40<"));
        assert!(
            m.contains(r#"<option value="0" selected>Off</option>"#),
            "{m}"
        );
        assert!(m.contains(r#"<a class="btn sm" href="""#));
    }
}
