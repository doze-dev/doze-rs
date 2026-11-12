//! Segmented control: a short row of mutually exclusive options that are LINKS (a filter that changes the address and is
//! applied in place, or a plain link with no script). One stop in the Tab order; arrows, Home and End move focus.

use maud::{Markup, html};

/// One segment.
#[derive(Debug, Clone, Copy)]
pub struct SegItem<'a> {
    pub label: &'a str,
    pub href: &'a str,
    pub current: bool,
}

/// The control. With `in_place` set to the id of the region the segments change, a click swaps only that region (and pushes the
/// address); without it a segment is a plain link. `label` names the group.
#[must_use]
pub fn seg(label: &str, items: &[SegItem<'_>], in_place: Option<&str>) -> Markup {
    html! {
        div class="seg dz-seg" role="group" aria-label=(label) x-data="dzSeg" x-on:keydown="key" x-on:focusin="focus" data-ui="seg" {
            @for it in items {
                @let target = in_place.map(|t| format!("#{t}"));
                a class={"dz-seg-i" @if it.current { " on" }} href=(it.href) data-seg aria-current=[it.current.then_some("true")]
                    hx-get=[target.as_ref().map(|_| it.href)] hx-target=[target.as_deref()] hx-select=[target.as_deref()]
                    hx-swap=[target.as_ref().map(|_| crate::ui::client::IN_PLACE_SWAP)] hx-push-url=[target.as_ref().map(|_| "true")] data-nopreload=[target.as_ref().map(|_| "false")] { (it.label) }
            }
        }
    }
}
