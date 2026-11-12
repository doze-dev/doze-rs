//! The four boards of the Access page rendered from the larder with no broker, as whole documents: what
//! `tests/boards_access.rs` writes to `target/board-diff/<id>.html` for `tools/board-diff.ts`. The two dialog boards are drawn
//! as a direct visit draws them: the Access page of the secured broker with the dialog open on it.

use maud::Markup;

use crate::assets::Assets;
use crate::base::Base;
use crate::fixtures_access as larder;
use crate::gather::access_dialogs::AclFields;
use crate::pages;
use crate::ui::overlay::{Over, over};
use crate::ui::shell::{Chrome, Crumb, Page, Rail, document_over};

/// The boards this builder renders, by screen id.
pub const IDS: [&str; 4] = [
    "access-open",
    "access-secured",
    "access-acl-create",
    "access-acl-delete",
];

/// The top bar of the secured broker: the chip says what is on (the open one is `fixtures::chrome()`).
fn chrome(secured: bool) -> Chrome {
    let mut c = crate::fixtures::chrome();
    if secured {
        c.security = "SASL SCRAM · ACLs + TLS".to_owned();
        c.locked = true;
    }
    c
}

fn secured(base: &Base) -> Markup {
    pages::access_page(&larder::page(true), base)
}

fn board(id: &str, base: &Base) -> Option<Over> {
    let plain = |main: Markup| Some(over(main, maud::html! {}));
    match id {
        "access-open" => plain(pages::access_page(&larder::page(false), base)),
        "access-secured" => plain(secured(base)),
        "access-acl-create" => {
            let f = AclFields {
                kind: "topic",
                name: "orders",
                pattern: "literal",
                principal: "User:orders",
                host: "*",
                op: "write",
                perm: "allow",
            };
            let v = larder::create_form(&f);
            Some(over(secured(base), pages::acl_create_dialog(&v, base)))
        }
        "access-acl-delete" => {
            let v = larder::delete_form(larder::acls().into_iter().next()?);
            Some(over(secured(base), pages::acl_delete_dialog(&v, base)))
        }
        _ => None,
    }
}

/// The page of a board, as a full document; `None` for an id that is not one of these.
#[must_use]
pub fn render(id: &str, base: &Base) -> Option<String> {
    let main = board(id, base)?;
    let assets = Assets::build(base);
    let chrome = chrome(id != "access-open");
    let crumbs = vec![
        Crumb {
            label: "Home".to_owned(),
            href: Some(base.url("/")),
        },
        Crumb {
            label: "Access".to_owned(),
            href: None,
        },
    ];
    let page = Page {
        base,
        assets: &assets.urls,
        chrome: &chrome,
        active: Rail::Access,
        crumbs,
        title: "Access",
        watch: &[],
    };
    Some(document_over(&page, main).into_string())
}
