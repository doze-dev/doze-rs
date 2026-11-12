//! Jump to a topic or a group: the palette (an overlay a script opens with ⌘K or Ctrl+K, or by a click on the top bar's search)
//! and its page (the same list as links, for a browser with no script). The names are the broker's own, from Metadata and
//! ListGroups; every one is text, and each carries a visible tag saying which it is.

use maud::{Markup, html};

use crate::base::{Base, segment};
use crate::ui::glyph::{Glyph, glyph};
use crate::ui::glyph_groups::{GroupGlyph, group_glyph};
use crate::ui::inplace;
use crate::ui::layout::page_header;
use crate::ui::overlay::{Kind, transient};

/// The region of the page: the filter and the names it leaves.
const JUMP_VIEW: &str = "jump-view";

/// The most names the palette lists; the page says when there are more.
pub(super) const MOST: usize = 500;

/// What a name in the palette is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JumpKind {
    Topic,
    Group,
    /// A place of the console itself (Home, Live, Connect, the two lists).
    Page,
}

/// The pages of the console the palette can go to, by the name it shows.
const PAGES: [(&str, &str); 5] = [
    ("Home", "/"),
    ("Live", "/live"),
    ("Connect", "/connect"),
    ("Topics", "/topics"),
    ("Groups", "/groups"),
];

/// One place the palette can go: a topic or a group, by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JumpItem {
    pub kind: JumpKind,
    pub name: String,
}

impl JumpItem {
    /// The topics, then the groups, as the palette lists them.
    #[must_use]
    pub fn all(topics: &[String], groups: &[String]) -> Vec<Self> {
        let of = |kind| {
            move |n: &String| Self {
                kind,
                name: n.clone(),
            }
        };
        let pages: Vec<String> = PAGES.iter().map(|(n, _)| (*n).to_owned()).collect();
        topics
            .iter()
            .map(of(JumpKind::Topic))
            .chain(groups.iter().map(of(JumpKind::Group)))
            .chain(pages.iter().map(of(JumpKind::Page)))
            .collect()
    }

    const fn word(&self) -> &'static str {
        match self.kind {
            JumpKind::Topic => "topic",
            JumpKind::Group => "group",
            JumpKind::Page => "page",
        }
    }

    fn href(&self, base: &Base) -> String {
        let at = match self.kind {
            JumpKind::Topic => "topics",
            JumpKind::Group => "groups",
            JumpKind::Page => {
                let path = PAGES
                    .iter()
                    .find(|(n, _)| *n == self.name)
                    .map_or("/", |(_, p)| p);
                return base.url(path);
            }
        };
        base.url(&format!("/{at}/{}", segment(&self.name)))
    }
}

/// The names, in three sections with a heading each (Topics, Groups, Pages); a section with no name has no heading.
fn list(items: &[JumpItem], base: &Base) -> Markup {
    let shown: Vec<&JumpItem> = items.iter().take(MOST).collect();
    let section = |kind: JumpKind, title: &'static str| -> Markup {
        let of: Vec<&&JumpItem> = shown.iter().filter(|i| i.kind == kind).collect();
        html! {
            @if !of.is_empty() {
                li class="jump-h" role="presentation" { (title) }
                @for i in of {
                    li { a class="jump-item" href=(i.href(base)) data-jump=(i.name.to_lowercase()) {
                        @match i.kind {
                            JumpKind::Topic => (glyph(Glyph::Topic, 16, "")),
                            JumpKind::Group => (group_glyph(GroupGlyph::Group, 16, "")),
                            JumpKind::Page => (glyph(Glyph::Home, 16, "")),
                        }
                        span class="mono" { (i.name) }
                        span class="jump-kind" { (i.word()) }
                    } }
                }
            }
        }
    };
    html! {
        ul class="jump-list" id="jump-list" aria-label="Topics, groups and pages" {
            (section(JumpKind::Topic, "Topics"))
            (section(JumpKind::Group, "Groups"))
            (section(JumpKind::Page, "Pages"))
        }
    }
}

fn more(total: usize) -> Markup {
    html! { @if total > MOST { p class="help" { "Showing the first " (MOST) " of " (total) " names. Type a longer name to narrow them." } } }
}

/// The palette: a field, the topics, one line of keys. Opened in place by the script; it has no parent page of its own.
#[must_use]
pub fn jump_overlay(items: &[JumpItem], total: usize, base: &Base) -> Markup {
    transient(
        Kind::Dialog,
        "jump-title",
        &base.url("/jump"),
        html! {
            div class="jump" {
                h2 id="jump-title" class="sr" { "Jump to a topic or group" }
                div class="input" {
                    (glyph(Glyph::Search, 16, ""))
                    input id="jump-q" type="text" placeholder="Jump to a topic or group" aria-label="Jump to a topic or group" autocomplete="off" spellcheck="false" data-jump-input;
                }
                (list(items, base))
                p class="help jump-empty" hidden { "No topic or group is named like that." }
                @if items.is_empty() { p class="help" { "This broker holds no topics or groups yet." } }
                (more(total))
                p class="help jump-keys" { kbd { "↑" } " " kbd { "↓" } " move " kbd { "Enter" } " open " kbd { "Esc" } " close" }
            }
        },
    )
}

/// The same list as a page, for a browser with no script: a plain GET filter and plain links.
#[must_use]
pub fn jump_page(items: &[JumpItem], total: usize, filter: &str, base: &Base) -> Markup {
    html! {
        (page_header("Jump to a topic or group", html! { p class="sub" { "Every topic and every group of this broker, by name. A topic page has its Messages, Partitions and the rest; a group page its members and lag." } }, html! {}))
        (inplace::region(JUMP_VIEW, html! {
            form class="toolbar jump-form" method="get" action=(base.url("/jump")) role="search" hx-get=(base.url("/jump")) hx-target={"#" (JUMP_VIEW)} hx-select={"#" (JUMP_VIEW)} hx-swap=(crate::ui::client::IN_PLACE_SWAP) hx-push-url="true" hx-trigger="submit" {
                div class="input mono" {
                    (glyph(Glyph::Search, 16, ""))
                    (inplace::search_input(&inplace::Search { target: JUMP_VIEW, action: &base.url("/jump"), name: "q", id: Some("q"), placeholder: "Part of a name", label: "Part of a topic or group name", value: filter }))
                }
                button class="btn" type="submit" { "Filter" }
            }
            div class="jump-page" {
                @if items.is_empty() { p class="help" { "No topic or group is named like that." } } @else { (list(items, base)) }
                (more(total))
            }
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn many(n: usize) -> Vec<JumpItem> {
        (0..n)
            .map(|i| JumpItem {
                kind: if i % 2 == 0 {
                    JumpKind::Topic
                } else {
                    JumpKind::Group
                },
                name: format!("n{i}"),
            })
            .collect()
    }

    #[test]
    fn a_hostile_name_is_text_in_a_link_made_by_segment_and_the_tag_says_which_it_is() {
        let hostile = "<img src=x onerror=alert(1)>/../\"";
        let items = JumpItem::all(&[hostile.to_owned()], &[hostile.to_owned()]);
        let m = jump_overlay(&items, 2, &Base::root()).into_string();
        assert!(!m.contains("<img"), "{m}");
        assert!(
            m.contains("href=\"/topics/%3Cimg") && m.contains("href=\"/groups/%3Cimg"),
            "{m}"
        );
        assert!(
            m.contains("<span class=\"jump-kind\">topic</span>")
                && m.contains("<span class=\"jump-kind\">group</span>")
        );
    }

    #[test]
    fn the_palette_lists_the_most_it_may_and_says_so() {
        let m = jump_overlay(&many(MOST + 5), MOST + 5, &Base::root()).into_string();
        assert_eq!(m.matches("class=\"jump-item\"").count(), MOST);
        assert!(m.contains("Showing the first 500 of 505 names"));
        let few = jump_overlay(&many(3), 3, &Base::root()).into_string();
        assert!(!few.contains("Showing the first"));
    }
}
