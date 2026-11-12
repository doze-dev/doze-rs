//! The client layer: EVERY attribute, id and event name that the browser-side script reads lives in this one file.
//!
//! The pages are server-rendered and correct with JavaScript off: each fragment is also a plain GET, each form a plain
//! POST. What a script adds is only enhancement: fragments that refresh when the server says they changed, forms that
//! swap their result in place, copy buttons. The script is `console.js`, ours (no library: about 80 lines over `EventSource` and
//! `fetch`, chosen by the D4a spike). If the client layer is ever replaced, this file and `assets/console.js` change, and no page
//! does. The pages emit plain data-attributes and ids; the script reads them. A test checks that `console.js` uses the names defined here.

use maud::{Markup, html};

/// The server-sent event that names the fragments that changed.
pub const EVENT_TICK: &str = "tick";
/// The attribute of a fragment that holds the URL it is fetched from.
pub const ATTR_FRAG: &str = "data-frag";
/// A form the script posts in place (the server answers with the fragment named by `data-target`).
pub const ATTR_AJAX: &str = "data-ajax";
/// The id of the fragment a posted form's answer replaces.
pub const ATTR_TARGET: &str = "data-target";
/// The request header that says "I want the fragment, not a redirect".
pub const HEADER_FRAGMENT: &str = "x-fragment";
/// The attribute of `<body>` that holds the URL of the stream.
pub const ATTR_STREAM: &str = "data-stream";
/// A control that only works with a script: hidden until the script reveals it.
pub const ATTR_JS_ONLY: &str = "data-js-only";
/// The text a copy button puts on the clipboard.
pub const ATTR_COPY: &str = "data-copy";
/// The id of the element whose text a copy button puts on the clipboard.
pub const ATTR_COPY_FROM: &str = "data-copy-from";
/// A button that sets what the page's copy buttons copy: `code`, `properties` or `environment`.
pub const ATTR_COPY_AS: &str = "data-copy-as";
/// A copy button whose text follows the "Copy as" control (the client snippets); a copy button without it copies its own text.
pub const ATTR_COPY_FORMS: &str = "data-copy-forms";
/// The address (bootstrap) the alternative copy forms are built from.
pub const ATTR_ADDRESS: &str = "data-address";
/// A link whose target is an overlay (a drawer or a dialog): with a script it opens over the page, without one it is an
/// ordinary link to the parent page with the overlay open on it.
pub const ATTR_DIALOG: &str = "data-dialog";
/// A control inside an overlay that closes it (Cancel, Close): without a script it is a link to the parent page.
pub const ATTR_DIALOG_CLOSE: &str = "data-dialog-close";
/// The top bar's search: its value is the address of the jump palette, which a click or ⌘K / Ctrl+K opens in place.
pub const ATTR_PALETTE: &str = "data-palette";
/// An overlay with no page of its own to return to (the palette): closed with no history entry.
pub const ATTR_TRANSIENT: &str = "data-transient";
/// The palette's field, and what each of its links carries to be matched against (the lower-cased name).
pub const ATTR_JUMP_INPUT: &str = "data-jump-input";
pub const ATTR_JUMP: &str = "data-jump";
/// The attribute of the overlay element (`ui::overlay::frame`); its value-less presence is how the script finds it.
pub const ATTR_OVERLAY: &str = "data-overlay";
/// The response header that says the body is an overlay element (and not a page): a script puts it in place.
pub const HEADER_OVERLAY: &str = "x-overlay";
/// The classes the script puts on an overlay while it comes in and while it goes out (the CSS animates them).
pub const CLASS_OVERLAY_IN: &str = "ovl-in";
pub const CLASS_OVERLAY_OUT: &str = "ovl-out";
/// The class of an element that is a fragment of its own.
pub const CLASS_FRAGMENT: &str = "frag";
/// The id of the banner shown while the stream is lost.
pub const ID_BANNER: &str = "stream-banner";
/// The id of the overlay root: the element every overlay is put in (the target of the requests that open one).
pub const ID_OVERLAY_ROOT: &str = "ovl";
/// htmx's configuration, as the document says it (a `<meta>`, so that no inline script is needed). Why each line is there:
/// `allowEval` off and `allowScriptTags` off keep the CSP (no `unsafe-eval`, no inline script) true of htmx; `selfRequestsOnly`
/// refuses a request to another origin; `historyCacheSize` 0 keeps page snapshots (message values!) out of `localStorage` and
/// always restores a fresh page; `includeIndicatorStyles` off because the console draws its own; `scrollIntoViewOnBoost` off
/// because the swap says where to scroll; every status except 204 swaps, so that a refusal (422) and a 404 page are shown.
pub const HTMX_CONFIG: &str = r#"{"allowEval":false,"allowScriptTags":false,"selfRequestsOnly":true,"historyCacheSize":0,"includeIndicatorStyles":false,"scrollIntoViewOnBoost":false,"defaultSwapStyle":"outerHTML","responseHandling":[{"code":"204","swap":false},{"code":".*","swap":true}]}"#;
/// What goes in `<head>` for the client layer: htmx's configuration and the scripts, in load order (all deferred).
#[must_use]
pub fn head(script_urls: &[String]) -> Markup {
    html! {
        meta name="htmx-config" content=(HTMX_CONFIG);
        @for u in script_urls { script src=(u) defer {} }
    }
}

/// `<body>`: every link and form in it is boosted (a script swaps `main` and keeps the shell), they work as plain ones without
/// one. A page swap cross-fades (`transition:true`) and starts at the top; a control that changes only the data it owns says
/// so with [`in_place_link`] or [`in_place_form`], and the preload extension fetches a link's page when the pointer or focus shows intent.
#[must_use]
pub fn body(content: Markup) -> Markup {
    html! {
        body hx-boost="true" hx-target="#main" hx-swap="outerHTML transition:true show:window:top" hx-ext="morph,preload" preload="mouseover" { (content) }
    }
}

/// How a control that changes only the region `target_id` of its own page asks for it: the answer is the page's region, the part
/// with that id is picked out of it and morphed into place (focus, typed text and scroll stay where they are), the address is
/// pushed, and nothing cross-fades. `target_id` is a literal of the page, never data.
pub const IN_PLACE_SWAP: &str = "morph:outerHTML show:none";

/// A link that changes only the region `target_id` (a chip, a sort, a pager): a plain link with no script.
#[must_use]
pub fn in_place_link(target_id: &str, class: &str, href: &str, content: Markup) -> Markup {
    html! {
        a class=(class) href=(href) hx-get=(href) hx-target={"#" (target_id)} hx-select={"#" (target_id)} hx-swap=(IN_PLACE_SWAP) hx-push-url="true" preload="false" { (content) }
    }
}

/// A GET form that changes only the region `target_id` (a toolbar's filters): a plain GET with no script. A control in it with
/// `data-autosubmit` submits it on change.
#[must_use]
pub fn in_place_form(target_id: &str, class: &str, action: &str, content: Markup) -> Markup {
    html! {
        form class=(class) method="get" action=(action) hx-get=(action) hx-target={"#" (target_id)} hx-select={"#" (target_id)} hx-swap=(IN_PLACE_SWAP) hx-push-url="true" hx-trigger="submit" { (content) }
    }
}

/// A part of a page that can be fetched alone at `url` and is fetched again when the stream says `id` changed.
/// Without a script it is just its content.
#[must_use]
pub fn fragment(id: &str, url: &str, content: Markup) -> Markup {
    html! {
        div class=(CLASS_FRAGMENT) id=(id) data-frag=(url) { (content) }
    }
}

/// A part of a page that is fetched alone but only refreshed by a person (a form's result): no stream trigger.
#[must_use]
pub fn fragment_static(id: &str, url: &str, content: Markup) -> Markup {
    html! { div class=(CLASS_FRAGMENT) id=(id) data-frag=(url) { (content) } }
}

/// A form that posts to `action` with no script (the server redirects back), and with one swaps the answer in
/// place of the fragment `target`.
#[must_use]
pub fn form(action: &str, target: &str, hidden: &[(&str, &str)], content: Markup) -> Markup {
    html! {
        form method="post" action=(action) data-ajax data-target=(target) hx-post=(action) hx-target={"#" (target)} hx-select={"#" (target)} hx-swap="morph:outerHTML show:none" hx-headers=r#"{"X-Fragment":"1"}"# {
            @for (name, value) in hidden { input type="hidden" name=(name) value=(value); }
            (content)
        }
    }
}

/// A button that copies `text`; only shown when a script can do it.
#[must_use]
pub fn copy_button(class: &str, text: &str, label: Markup) -> Markup {
    html! { button class=(class) type="button" data-ui="copy" data-copy=(text) data-js-only hidden { (label) } }
}

/// A link to a dialog's address: a plain link, marked so a script can open the dialog over the page.
#[must_use]
pub fn dialog_link(class: &str, href: &str, label: Markup) -> Markup {
    html! { a class=(class) href=(href) data-dialog { (label) } }
}

/// The address chip of the top bar: always shown (it is the address), and copies it when a script can.
#[must_use]
pub fn copy_chip(text: &str, label: Markup) -> Markup {
    html! { button class="chip" type="button" data-copy=(text) title="Copy the address" aria-label=(format!("Copy address {text}")) { (label) } }
}

/// A button that copies the text of the element `target_id`.
#[must_use]
pub fn copy_from_button(class: &str, target_id: &str, label: Markup) -> Markup {
    html! { button class=(class) type="button" data-ui="copy" data-copy-from=(target_id) data-js-only hidden { (label) } }
}

/// A button that copies the text of the element `target_id` in the form "Copy as" says (the client snippets).
#[must_use]
pub fn copy_snippet_button(class: &str, target_id: &str, label: Markup) -> Markup {
    html! { button class=(class) type="button" data-ui="copy" data-copy-from=(target_id) data-copy-forms data-js-only hidden { (label) } }
}

/// A button of the "Copy as" control: sets what the copy buttons of the page copy.
#[must_use]
pub fn copy_as_button(kind: &str, on: bool, label: &str) -> Markup {
    html! { button class=(super::filter::seg_class(on)) type="button" data-ui="tab" data-copy-as=(kind) data-js-only hidden { (label) } }
}

/// The element that carries the address the alternative copy forms are built from.
#[must_use]
pub fn address_holder(address: &str, content: Markup) -> Markup {
    html! { div data-address=(address) { (content) } }
}

/// The stream's URL for a set of fragment ids.
#[must_use]
pub fn stream_url(events_path: &str, ids: &[&str]) -> String {
    format!("{events_path}?watch={}", ids.join(","))
}

/// The one inline piece of script-free styling the client layer needs: a hidden control stays hidden.
pub const CSS: &str = "[hidden]{display:none!important}.frag{display:contents}";

#[cfg(test)]
mod tests;
