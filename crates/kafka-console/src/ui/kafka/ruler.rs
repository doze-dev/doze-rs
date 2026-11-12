//! The offsets ruler of a partition: where each reader stops, over the offsets just below the end.

use maud::{Markup, html};

use crate::format::number;

/// A marker on the ruler: its name, the offset, and where along the ruler it sits, in per cent (0 to 100).
#[derive(Debug, Clone, Copy)]
pub struct Mark<'a> {
    pub name: &'a str,
    pub class: &'a str,
    pub offset: i64,
    pub at: f64,
}

/// The ruler for offsets `from` to `to`. The part below `stable` is read by both isolation levels; the part from `stable` to
/// `to` only by `read_uncommitted`. The bar sits at `stable`.
#[must_use]
pub fn ruler(from: i64, stable: i64, to: i64, marks: &[Mark<'_>]) -> Markup {
    let below = (stable - from).max(0);
    let held = (to - stable).max(0);
    html! {
        div class="ruler" data-extra="ruler" style="border:0;padding:0" {
            div class="mks" {
                @for m in marks {
                    div class={"mk " (m.class)} style={"--at:" (format!("{:.1}", m.at)) "%" @if m.at >= 100.0 { ";transform:translateX(-100%)" }} {
                        span { (m.name) } b { (number(m.offset)) }
                    }
                }
            }
            div class="segs" {
                @if below > 0 { div class="sg sealed" style={"flex:" (below)} { span { (number(from)) " to " (number(stable - 1)) } } }
                @if held > 0 { div class="sg active" style={"flex:" (held)} { span { (number(stable)) " to " (number(to - 1)) ": held" } } }
            }
            div class="axis" { span { (number(from)) } span { (number(to)) } }
            div class="key" {
                span { i style="background:#eef3fd" {} "read by both" }
                span { i style="background:#ece6fb" {} "only read_uncommitted" }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ruler_splits_the_offsets_at_the_last_stable_one() {
        let marks = [
            Mark { name: "LSO", class: "lso", offset: 16_098, at: 57.1 },
            Mark { name: "HW", class: "hw", offset: 16_104, at: 100.0 },
        ];
        let m = ruler(16_090, 16_098, 16_104, &marks).into_string();
        assert!(m.contains(r#"style="flex:8"><span>16,090 to 16,097</span>"#), "{m}");
        assert!(m.contains(r#"style="flex:6"><span>16,098 to 16,103: held</span>"#), "{m}");
        assert!(m.contains("--at:100.0%;transform:translateX(-100%)"), "{m}");
        assert!(m.contains("<b>16,098</b>"));
    }

    #[test]
    fn a_partition_with_nothing_held_has_no_held_part() {
        let m = ruler(0, 10, 10, &[]).into_string();
        assert!(!m.contains("held"), "{m}");
    }
}
