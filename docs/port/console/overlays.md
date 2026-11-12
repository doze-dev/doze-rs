# Overlays: drawers and dialogs over their parent page, and how the console moves

The owner's rule (2026-11-13): a board of kind `drawer` or `dialog` is NOT a page of its own. It is drawn over its parent page, so it is built that way: one record, one produce form, one delete confirmation opens over the page it came from, and the page behind stays where it is. The pages that are pages (kind `page` or `state`) stay pages.

## The contract

| | With JavaScript | Without JavaScript (a direct visit, a reload, a shared link) |
|---|---|---|
| Open | a link with `data-dialog` fetches the overlay alone (`X-Fragment: 1`; the answer carries `X-Overlay: 1`), puts it over the current page, `history.pushState` to the overlay's address; nothing is navigated | the overlay's address answers the PARENT page's whole document with the overlay already open on it (the parent is `inert` and the page does not scroll, by markup and CSS alone) |
| Close | Esc, a click on the scrim, the Close / Cancel control, browser Back: the address goes back, scroll is restored, focus returns to the opener | the scrim and the Close / Cancel control are plain links to the parent's address |
| Next record | a second opener click replaces the content in place (no new history entry, no coming-in again) | a link |
| A form in it | posts in place: an overlay that comes back (a refusal, a field to fix) replaces this one; a page that comes back (the result of a write) replaces the page, and the address is the parent's (or the redirect's) | a normal POST whose answer is the result page, as the boards draw it |
| Focus and reading | focus goes to the first field (or the panel, which is named by its title), Tab and Shift+Tab stay inside, `aria-modal`, `role=dialog`, `aria-labelledby`; the page behind is `inert` and the page does not scroll | the same markup; `inert` is the browser's own |

A server-rendered overlay is adopted by the script at load: the same Esc, scrim, Close and Back, with no fetch.

## Motion

Opening: the scrim fades in 160 ms; a drawer slides in 220 ms (`translateX(28px)` to 0 with opacity, `cubic-bezier(.2,.7,.3,1)`); a dialog fades and rises 8 px in 180 ms. Closing is the reverse, 140 ms, ease-in. Nothing bounces. Only `opacity` and `transform` move. Everything is inside `@media (prefers-reduced-motion: no-preference)`; with reduced motion the overlay appears and goes at once (the script checks `matchMedia` too, so it does not wait for an `animationend` that never comes).

Page to page: a cross-document view transition (`@view-transition { navigation: auto }`; Chrome and Safari 18, other browsers simply navigate). The top bar and the rail have their own `view-transition-name`, so they stay still (only the active rail item cross-fades); the page area (`.main`) fades in 180 ms with a 4 px rise, the old one out in 120 ms. A write's result page that replaces an overlay's page uses the same-document `startViewTransition` where it exists.

Fragments that the stream refreshes: a node whose text changed, or that was added or removed, fades from 0.45 to 1 opacity in 120 ms (Web Animations); an unchanged answer (the 304 path) changes nothing and animates nothing. Hover, focus and pressed states of buttons, rows, tabs and the rail answer in 120 ms (colour, border and shadow only).

## How to add an overlay (the recipe)

1. **The body**, a pure function in the page's family: `fn xyz_dialog(view: &XyzView, base: &Base) -> Markup`. For a dialog call `ui::dialog::modal(id, action, close_href, title, body, foot)` (one POST form; the title is `{id}-title` and names the panel); for a drawer call `ui::overlay::frame(Kind::Drawer, "xyz-title", &close_href, html! { h2 id="xyz-title" { ... } ... })`. `close_href` is the PARENT page's address (with its query if it has one). Put a `ui::overlay::close("btn", &close_href, ...)` (or `dialog::cancel`) in the foot so that Close is a link that a script takes over.
2. **The parent**, a builder that returns `handlers::overlays::Parent` (rail, title, breadcrumb, stream ids, `main`), made from the same code as the parent's own page handler: `handlers::topics::messages_parent` and `partitions_parent` are the examples; the parent's own handler is `page_of(parent)`.
3. **The route**, a thin handler:
   ```rust
   let overlay = pages::xyz_dialog(&view, &s.base);
   if overlays::wants_fragment(&headers) { return overlays::fragment(StatusCode::OK, &overlay); }
   let Some(parent) = xyz_parent(&s, &topic).await else { return no_topic(&s, &topic).await };
   overlays::over_parent(&s, StatusCode::OK, parent, overlay).await
   ```
   A POST answers the same way for a refusal that the board draws in the dialog (status 422 for an invalid field); a result that the board draws as a page (`-saved`, `-created`, `-deleted`, a refusal drawn as a page) is a normal page response.
4. **The opener**: on the parent page use `ui::button::opener(href, variant, size, glyph, label)` for a button, or `ui::client::dialog_link(class, href, label)` for a link. Without a script it is an ordinary link to the overlay's address.
5. **The route table**: `routes.rs` lists the overlay's address under its board ids as always; `design/routes.json` is regenerated.
6. **The board render**: in `boards*.rs` render the parent's fixture page and put the overlay on it with `ui::overlay::over(parent_main, overlay)` and `ui::shell::document_over`. The board-diff signature of an overlay board is the dialog element only.
7. **Tests**: a direct visit contains the parent's heading and the open overlay (`tests/overlays.rs` has the table to extend: add the address, the parent's heading and the parent's address that closing returns to), a fragment request gets the element alone with `x-overlay`, a hostile name is inert.

The script reads these names, all defined in `ui/client.rs` and checked by a test: `data-dialog` (opener), `data-dialog-close` and `data-overlay-close` (close controls), `data-overlay` and `data-close` (the element and its parent's address), the `x-overlay` header, the classes `ovl-in` and `ovl-out`.

## Where it is built

`ui/overlay.rs` (kinds, `frame`, `over`, `close`), `ui/dialog.rs` (the dialog's form), `ui/shell.rs` (`document_over`: `inert` and the overlay after the page), `handlers/overlays.rs` (`Parent`, `fragment`, `over_parent`), `assets/console.js` (open, close, history, focus, form answers, page swap), `design/console.css` (overlay, view transitions, states), tests: `tests/overlays.rs`, `tools/dom/overlay.test.ts`.
