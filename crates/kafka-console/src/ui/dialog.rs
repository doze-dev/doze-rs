//! Dialogs: a centred overlay (`ui::overlay`) whose content is one `<form method=post>`, so submitting it works with no
//! script. With a script a link marked `data-dialog` opens it over the page it was clicked on and its answer replaces it in
//! place; without one the same address is the parent page with the dialog open on it.

use maud::{Markup, html};

use super::overlay::{Kind, frame};

/// The dialog: a title, a body, a foot of buttons, all inside one POST form to `action`, framed as an overlay that closes
/// to `close_href` (its parent page).
///
/// `id` names the dialog: its title is `{id}-title` and the panel is labelled by it (`aria-labelledby`).
#[must_use]
pub fn modal(
    id: &str,
    action: &str,
    close_href: &str,
    title: Markup,
    body: Markup,
    foot: Markup,
) -> Markup {
    let title_id = format!("{id}-title");
    frame(
        Kind::Dialog,
        &title_id,
        close_href,
        html! {
            form method="post" action=(action) id=(id) {
                h2 id=(title_id) { (title) }
                div class="body" { (body) }
                div class="foot" { (foot) }
            }
        },
    )
}

/// The Cancel link of a dialog's foot: it goes back to the parent page, and with a script it closes the dialog instead.
#[must_use]
pub fn cancel(href: &str) -> Markup {
    super::overlay::close("btn", href, html! { "Cancel" })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dialog_is_labelled_by_its_title_and_is_one_form() {
        let m = modal(
            "produce",
            "/topics/orders/produce",
            "/topics/orders",
            html! { "Produce to " span class="mono" { "orders" } },
            html! { "body" },
            html! { "foot" },
        )
        .into_string();
        assert!(
            m.contains(
                r#"<div class="modal" role="dialog" aria-modal="true" aria-labelledby="produce-title" tabindex="-1"><form method="post" action="/topics/orders/produce" id="produce"><h2 id="produce-title">Produce to <span class="mono">orders</span></h2><div class="body">body</div><div class="foot">foot</div></form></div>"#
            ),
            "{m}"
        );
        assert!(m.contains(r#"<a class="scrim" href="/topics/orders""#));
    }

    #[test]
    fn cancel_is_a_link_back_that_a_script_turns_into_a_close() {
        assert_eq!(
            cancel("/topics/orders").into_string(),
            r#"<a class="btn" href="/topics/orders" data-dialog-close>Cancel</a>"#
        );
    }
}
