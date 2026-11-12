//! Dialogs: a route that returns the dialog's body. With a script a link marked `data-dialog` opens it in a `<dialog>`
//! over the page it was clicked on; without one the same address is a full page that shows the dialog in the page. The
//! dialog is one `<form method=post>` either way, so submitting it works with no script.

use maud::{Markup, html};

/// The dialog: a title, a body, a foot of buttons, all inside one POST form to `action`.
///
/// `id` ties the title to the dialog (`aria-labelledby`). `inline` is the full-page rendering: the dialog sits in the
/// page instead of floating over it.
#[must_use]
pub fn modal(
    id: &str,
    action: &str,
    inline: bool,
    title: Markup,
    body: Markup,
    foot: Markup,
) -> Markup {
    let title_id = format!("{id}-title");
    html! {
        div class={"modal" @if inline { " page" }} id=(id) role="dialog" aria-labelledby=(title_id) {
            form method="post" action=(action) {
                h2 id=(title_id) { (title) }
                div class="body" { (body) }
                div class="foot" { (foot) }
            }
        }
    }
}

/// The Cancel link of a dialog's foot: it goes back to the page, and with a script it closes the dialog instead.
#[must_use]
pub fn cancel(href: &str) -> Markup {
    html! { a class="btn" href=(href) data-dialog-close { "Cancel" } }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dialog_is_labelled_by_its_title_and_is_one_form() {
        let m = modal(
            "produce",
            "/topics/orders/produce",
            false,
            html! { "Produce to " span class="mono" { "orders" } },
            html! { "body" },
            html! { "foot" },
        )
        .into_string();
        assert_eq!(
            m,
            r#"<div class="modal" id="produce" role="dialog" aria-labelledby="produce-title"><form method="post" action="/topics/orders/produce"><h2 id="produce-title">Produce to <span class="mono">orders</span></h2><div class="body">body</div><div class="foot">foot</div></form></div>"#
        );
        assert!(
            modal("d", "/x", true, html! {}, html! {}, html! {})
                .into_string()
                .starts_with(r#"<div class="modal page""#)
        );
    }

    #[test]
    fn cancel_is_a_link_back_that_a_script_turns_into_a_close() {
        assert_eq!(
            cancel("/topics/orders").into_string(),
            r#"<a class="btn" href="/topics/orders" data-dialog-close>Cancel</a>"#
        );
    }
}
