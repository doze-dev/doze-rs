//! Tooltip and Popover. A tooltip is a few words for a truncated value or an icon button: shown after 400 ms of hover or focus,
//! hidden by leaving, blur or Esc; without a script it is the `title`. A popover is a small card opened by a button; without a
//! script its content is simply shown under the button.

use maud::{Markup, html};

use super::alpine;

/// A tooltip around `trigger` (a button, a link or text). `id` is unique on the page; `text` is plain text.
#[must_use]
pub fn tip(id: &str, text: &str, trigger: &Markup) -> Markup {
    alpine(html! {
        span class="dz dz-tip" x-data="dzTip" x-on:mouseenter="later" x-on:mouseleave="off" x-on:focusin="now" x-on:focusout="off" x-on:keydown="key" data-ui="tip" {
            span class="dz-tip-t" x-ref="trigger" title=(text) aria-describedby=(id) { (trigger) }
            span class="dz-pop dz-tip-pop" role="tooltip" id=(id) x-show="shown" style="display:none" xa-top-6="$refs.trigger" { (text) }
        }
    })
}

/// A popover: a button labelled `label` that opens a card with `body`. `id` is unique on the page.
#[must_use]
pub fn popover(id: &str, label: &str, body: &Markup) -> Markup {
    alpine(html! {
        div class="dz dz-pov" x-data="dzPopover" x-on:keydown="key" xo-click="away" x-on:focusout="focusout" xb-data-open="open" data-ui="popover" {
            button type="button" class="btn sm dz-pov-btn" x-ref="trigger" x-on:click="toggle" aria-haspopup="dialog" aria-expanded="false" xb-aria-expanded="open" aria-controls=(id) hidden { (label) }
            div class="dz-pop dz-pov-card" role="dialog" id=(id) aria-label=(label) x-show="open" xa-bottom-start-6="$refs.trigger" { (body) }
        }
    })
}
