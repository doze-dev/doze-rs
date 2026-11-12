//! The shell every page sits in: the top bar, the rail, the breadcrumb and the document around them.
//!
//! The boards draw this chrome once (`kit/build.ts`); this is its port. What the boards draw and the console does not
//! yet have is left out and said so in the code: the pinned items of the rail (no store holds them, REALITY 5.5), the
//! `j k` and `⌘K` hints (no keyboard layer yet), and the "Reached from" line (canvas scaffolding, REALITY 5.7).

use maud::{DOCTYPE, Markup, html};

use super::client;
use super::glyph::{Glyph, glyph};
use super::overlay::Over;
use crate::assets::AssetUrls;
use crate::base::Base;

/// A place in the rail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rail {
    Home,
    Live,
    Resources,
    Access,
    Lab,
    Settings,
}

impl Rail {
    const ALL: [Self; 6] = [
        Self::Home,
        Self::Live,
        Self::Resources,
        Self::Access,
        Self::Lab,
        Self::Settings,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::Home => "Home",
            Self::Live => "Live",
            Self::Resources => "Resources",
            Self::Access => "Access",
            Self::Lab => "Lab",
            Self::Settings => "Settings",
        }
    }

    const fn glyph(self) -> Glyph {
        match self {
            Self::Home => Glyph::Home,
            Self::Live => Glyph::Live,
            Self::Resources => Glyph::Grid,
            Self::Access => Glyph::Access,
            Self::Lab => Glyph::Flask,
            Self::Settings => Glyph::Settings,
        }
    }

    /// The entry of the space, as a route path.
    #[must_use]
    pub const fn path(self) -> &'static str {
        match self {
            Self::Home => "/",
            Self::Live => "/live",
            Self::Resources => "/topics",
            Self::Access => "/access",
            Self::Lab => "/lab",
            Self::Settings => "/settings",
        }
    }
}

/// What the top bar says: data from the broker, not decoration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chrome {
    /// The address a client is pointed at, `127.0.0.1:9092`.
    pub address: String,
    /// `Kafka 4 profile`.
    pub profile: String,
    /// `Open`, `Open · ACLs`, `SASL SCRAM + TLS`.
    pub security: String,
    /// Whether the security chip carries a lock (something is on) and not the open dot.
    pub locked: bool,
    /// The console is served beyond loopback with no login (`--console-unprotected`).
    pub unprotected: bool,
}

/// One step of the breadcrumb; the last has no link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Crumb {
    pub label: String,
    pub href: Option<String>,
}

/// Everything around a page's own content.
#[derive(Debug)]
pub struct Page<'a> {
    pub base: &'a Base,
    pub assets: &'a AssetUrls,
    pub chrome: &'a Chrome,
    pub active: Rail,
    pub crumbs: Vec<Crumb>,
    /// The document title and the page's name.
    pub title: &'a str,
    /// The fragment ids the page asks the stream to watch (none: no stream).
    pub watch: &'a [&'static str],
}

fn topbar(page: &Page<'_>) -> Markup {
    let c = page.chrome;
    html! {
        header class="topbar" {
            div class="brand" { span class="wm" { "doze-kafka" } }
            div class="search" data-palette=(page.base.url("/jump")) {
                (glyph(Glyph::Search, 16, ""))
                // Without a script the field is drawn, not offered (the Jump page has the same list as links); a script enables it
                // and opens the palette on a click or Enter, as ⌘K does.
                input type="text" placeholder="Jump to a topic or group" aria-label="Jump to a topic or group" disabled;
                kbd { "⌘K" }
            }
            div class="tbr" {
                (client::copy_chip(&c.address, html! { span class="mono" { (c.address) } (glyph(Glyph::Copy, 14, "")) }))
                span class="chip" { (c.profile) }
                a class="chip" href=(page.base.url(Rail::Access.path())) aria-label=(format!("Access: {}", c.security)) {
                    @if c.locked { (glyph(Glyph::Lock, 14, "")) } @else { i class="rec" {} }
                    (c.security)
                }
            }
        }
    }
}

fn rail(page: &Page<'_>) -> Markup {
    html! {
        nav class="rail" aria-label="Spaces" {
            div class="nav" {
                @for r in Rail::ALL {
                    @let on = r == page.active;
                    @let soon = crate::routes::is_planned(r.path());
                    a class={"ni" @if on { " on" } @if soon { " is-soon" }} href=(page.base.url(r.path())) aria-current=[on.then_some("page")] {
                        (glyph(r.glyph(), 20, "")) (r.label())
                        @if soon { span class="soon" { "Soon" } }
                    }
                }
            }
            // The keys the script answers to (the boards draw them): hidden when there is no script to answer.
            div class="hints" data-js-only hidden {
                span { kbd { "j" } kbd { "k" } "move" }
                span { kbd { "⌘K" } "jump" }
            }
        }
    }
}

fn crumbs(page: &Page<'_>) -> Markup {
    html! {
        nav class="crumbs" aria-label="Breadcrumb" {
            @for (i, c) in page.crumbs.iter().enumerate() {
                @if i > 0 { (glyph(Glyph::ChevR, 12, "")) }
                @if let Some(href) = &c.href { a href=(href) { (c.label) } } @else { span class="cur" { (c.label) } }
            }
        }
    }
}

/// The whole document: `main` is the page's own content.
#[must_use]
pub fn document(page: &Page<'_>, main: Markup) -> Markup {
    document_with(page, main, None)
}

/// A document with an overlay open on its parent page (`ui::overlay`): the parent is `inert` (no focus, no clicks, not read
/// out) and the page does not scroll, by the markup and the CSS alone; the overlay is the last thing in the body.
#[must_use]
pub fn document_over(page: &Page<'_>, over: Over) -> Markup {
    // An empty overlay is no overlay: a plain page goes through here too.
    let overlay = (!over.overlay.0.is_empty()).then_some(over.overlay);
    document_with(page, over.main, overlay)
}

fn document_with(page: &Page<'_>, main: Markup, overlay: Option<Markup>) -> Markup {
    let stream =
        (!page.watch.is_empty()).then(|| client::stream_url(&page.base.url("/events"), page.watch));
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                // A console on a developer's machine is not a page for a search engine.
                meta name="robots" content="noindex";
                title { (page.title) " · doze-kafka" }
                link rel="icon" type="image/svg+xml" href=(page.assets.favicon);
                link rel="stylesheet" href=(page.assets.css);
                (client::head(&page.assets.script))
            }
            (client::body(stream.as_deref(), html! {
                a class="skip" href="#main" { "Skip to the page" }
                div class="root" inert[overlay.is_some()] {
                    @if page.chrome.unprotected {
                        div class="banner bad" role="alert" {
                            b { "Served beyond loopback, no login." }
                            " Anyone who can reach this address can use the console, and the console acts on the broker as a super user."
                        }
                    }
                    (topbar(page))
                    div class="frame" {
                        (rail(page))
                        main class="main" id="main" {
                            (crumbs(page))
                            (super::notice::banner_bad(Some(client::ID_BANNER), true, html! {
                                b { "Live updates are paused." }
                                " The console lost its connection to the broker's process. It is trying again; the page itself is still right as of its last load."
                            }))
                            (main)
                        }
                    }
                }
                @if let Some(o) = &overlay { (o) }
            }))
        }
    }
}

#[cfg(test)]
mod tests;
