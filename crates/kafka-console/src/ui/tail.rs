//! The `Snapshot | Live tail` control of the two tails (CON-809): the segmented control, the connection indicator and the pill that
//! says how many new rows are held back. The behaviour is `assets/live.js` (`tailControl`); without a script the page is the
//! snapshot and the Live tail button stays disabled.

use maud::{Markup, html};

/// The control for the tail whose region is `region` (the id of the fragment that holds `.tail-root`).
#[must_use]
pub fn tail_bar(region: &str, available: bool) -> Markup {
    html! {
        div class="tailbar" x-data="tailControl" data-tail-bar data-tail-for=(region) {
            div class="seg" role="group" aria-label="Mode" {
                button type="button" class="on" aria-pressed="true" data-tail-mode="snapshot" x-on:click="snapshot" { "Snapshot" }
                button type="button" aria-pressed="false" data-tail-mode="live" x-on:click="live" title=[(!available).then_some("The tail follows the newest records: it is not offered with a key, a time or an offset")] data-unavailable[!available] disabled { "Live tail" }
            }
            span class="conn" data-conn="snapshot" role="status" aria-live="polite" { i class="dot" {} span data-conn-text { "Snapshot" } }
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
        assert!(m.contains(r#"data-tail-mode="live" x-on:click="live" title="" disabled"#));
        assert!(m.contains(r#"data-conn="snapshot""#) && m.contains("data-held-pill hidden"));
    }
}
