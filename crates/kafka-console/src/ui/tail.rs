//! The `Snapshot | Live tail` control of the two tails (CON-809): the segmented control, the connection indicator and the pill that
//! says how many new rows are held back. The behaviour is `assets/live.js` (`tailControl`); without a script the page is the
//! snapshot and the Live tail button stays disabled.

use maud::{Markup, html};

use super::components::{Choice, Select, select};

/// The speeds of a tail: the seconds between batches, and the name. Realtime is the fastest a tail goes (once a second).
pub const SPEEDS: [(&str, &str); 4] = [
    ("1", "Realtime"),
    ("3", "Relaxed"),
    ("10", "Slow"),
    ("0", "Paused"),
];
/// The speed a tail starts at.
pub const DEFAULT_SPEED: &str = "3";

/// The control for the tail whose region is `region` (the id of the fragment that holds `.tail-root`).
#[must_use]
pub fn tail_bar(region: &str, available: bool) -> Markup {
    let choices: Vec<Choice<'_>> = SPEEDS.iter().map(|(v, l)| Choice::new(v, l)).collect();
    html! {
        div class="tailbar" x-data="tailControl" x-on:change="speed" data-tail-bar data-tail-for=(region) {
            div class="seg" role="group" aria-label="Mode" {
                button type="button" class="on" aria-pressed="true" data-tail-mode="snapshot" x-on:click="snapshot" { "Snapshot" }
                button type="button" aria-pressed="false" data-tail-mode="live" x-on:click="live" title=[(!available).then_some("The tail follows the newest records: it is not offered with a key, a time or an offset")] data-unavailable[!available] disabled { "Live tail" }
            }
            span class="speed" title="Rows arrive in batches every 3 s" data-tail-speed {
                (select(&Select { class: "dz-chip", ui: Some("tail-speed"), ..Select::new("tail-speed", "speed", "Speed", &choices, DEFAULT_SPEED) }))
            }
            span class="tail-conn" data-conn="snapshot" role="status" aria-live="polite" { i class="dot" {} span data-conn-text { "Snapshot" } }
            button class="held-pill" type="button" data-held-pill hidden x-on:click="show" { "↑ " span data-held-n { "0" } " new · Show" }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_control_starts_as_a_snapshot_and_offers_the_tail_only_to_a_script() {
        let m = tail_bar("live-wire", true).into_string();
        assert!(m.contains(r#"data-tail-for="live-wire""#));
        assert!(m.contains(r#"class="on" aria-pressed="true" data-tail-mode="snapshot""#));
        assert!(m.contains(r#"data-tail-mode="live" x-on:click="live" disabled"#));
        assert!(m.contains(r#"data-conn="snapshot""#) && m.contains("data-held-pill hidden"));
    }
}
