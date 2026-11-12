//! DropdownMenu: a "..." button and a menu of actions. Without a script the items are shown in a row, as the links and buttons
//! they are, so every action stays reachable. Items are links (`href`) or buttons (a form's submit); a destructive one is
//! drawn in the danger colour; a separator is a rule.

use maud::{Markup, html};

use super::{Icon, alpine, icon};

/// One entry of a menu.
#[derive(Debug, Clone, Copy)]
pub enum MenuItem<'a> {
    /// A link, which may open an overlay (`dialog`: `data-dialog`).
    Link {
        label: &'a str,
        href: &'a str,
        dialog: bool,
        danger: bool,
    },
    /// A button of the form `form` (an id on the page) that submits it with `name=value`.
    Button {
        label: &'a str,
        form: &'a str,
        name: &'a str,
        value: &'a str,
        danger: bool,
    },
    Separator,
}

/// A menu.
#[derive(Debug, Clone, Copy)]
pub struct Menu<'a> {
    pub id: &'a str,
    /// The accessible name of the "..." button ("Actions for orders").
    pub label: &'a str,
    pub items: &'a [MenuItem<'a>],
}

fn item(m: &Menu<'_>, n: usize, it: &MenuItem<'_>) -> Markup {
    let id = format!("{}-i{n}", m.id);
    html! {
        @match it {
            MenuItem::Link { label, href, dialog, danger } => {
                a class={"dz-item" @if *danger { " danger" }} role="menuitem" id=(id) href=(href) data-dialog[*dialog] { (label) }
            }
            MenuItem::Button { label, form, name, value, danger } => {
                button class={"dz-item" @if *danger { " danger" }} role="menuitem" id=(id) type="submit" form=(form) name=(name) value=(value) { (label) }
            }
            MenuItem::Separator => { div class="dz-sep" role="separator" {} }
        }
    }
}

/// The menu.
#[must_use]
pub fn menu(m: &Menu<'_>) -> Markup {
    alpine(menu_html(m))
}

fn menu_html(m: &Menu<'_>) -> Markup {
    html! {
        div class="dz dz-menu" x-data="dzMenu" x-on:keydown="key" x-on:click="click" xo-click="away" x-on:focusout="focusout" xb-data-open="open" data-ui="menu" {
            button type="button" class="dz-menu-btn" x-ref="trigger" aria-haspopup="menu" aria-expanded="false" xb-aria-expanded="open" aria-controls=(format!("{}-menu", m.id)) aria-label=(m.label) hidden {
                (icon(Icon::Dots, ""))
            }
            div class="dz-pop dz-menu-list" role="menu" id=(format!("{}-menu", m.id)) aria-label=(m.label) x-ref="list" x-show="open" xa-bottom-end-4="$refs.trigger" {
                @for (n, it) in m.items.iter().enumerate() { (item(m, n, it)) }
            }
        }
    }
}
