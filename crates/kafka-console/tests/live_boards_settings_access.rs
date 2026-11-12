//! The eight boards of slice 9h, rendered from a real broker in this process and written to `target/board-diff-live/<id>.html`
//! for `tools/board-diff.ts` (structure only: the same columns, buttons, links and cards as the board, whatever the names and
//! numbers). The open pages come from an open broker, the Access ones from a broker with SCRAM, ACLs and a super user.
#![allow(
    clippy::too_many_lines,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;
mod support_access;
mod support_write;

use std::path::PathBuf;

use axum::http::StatusCode;
use support::get;
use support_access::{router_with, secured, start};
use support_write::{page, post_form, world};

fn out_dir() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest.parent().unwrap().parent().unwrap();
    let target =
        std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| root.join("target"), PathBuf::from);
    target.join("board-diff-live")
}

fn write(id: &str, doc: &str) {
    let dir = out_dir();
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{id}.html")), doc).unwrap();
}

/// CON-572: the Settings boards and the open Access board, from an open broker with topics.
#[tokio::test]
async fn the_settings_boards_and_the_open_access_board_render_from_a_real_broker() {
    let w = world("live-9h-open", |_, _| {}).await;
    w.create_topic("orders", 2, &[("retention.ms", "604800000")])
        .await;
    w.create_topic("payments.settled", 1, &[]).await;
    let mut known = start(false);
    known.flags = kafka_console::fixtures_settings::page().flags;
    let app = router_with(w.facts.clone(), known, w.backend.clone());
    let (status, doc) = page(&app, get("/settings")).await;
    assert_eq!(status, StatusCode::OK);
    write("settings-home", &doc);
    let (_, doc) = page(&app, get("/settings/config/edit?key=log.retention.ms")).await;
    write("broker-config-edit", &doc);
    let (_, doc) = page(
        &app,
        post_form("/settings/config/edit", &[("probe", "num.partitions")]),
    )
    .await;
    write("broker-config-refused", &doc);
    let f = [
        ("key", "log.retention.ms"),
        ("was", "log.retention.ms"),
        ("scope", "all"),
        ("value", "259200000"),
    ];
    let (_, doc) = page(&app, post_form("/settings/config/edit", &f)).await;
    write("broker-config-saved", &doc);
    let (status, doc) = page(&app, get("/access")).await;
    assert_eq!(status, StatusCode::OK);
    write("access-open", &doc);
    w.stop().await;
}

/// CON-573: the secured Access board and the two ACL dialogs, from a broker with SCRAM, a super user and ACLs.
#[tokio::test]
async fn the_secured_access_board_and_the_acl_dialogs_render_from_a_real_broker() {
    let s = secured("live-9h-secured", true).await;
    let app = s.router();
    for (name, user) in [
        ("orders", "User:orders"),
        ("orders", "User:payments"),
        ("payments.", "User:payments"),
        ("customer.profiles", "User:report-job"),
    ] {
        let mut f = vec![
            ("type", "topic"),
            ("name", name),
            (
                "pattern",
                if name.ends_with('.') {
                    "prefixed"
                } else {
                    "literal"
                },
            ),
            ("principal", user),
            ("host", "*"),
            ("op", "write"),
            ("perm", "allow"),
        ];
        if user == "User:report-job" {
            f[6] = ("perm", "deny");
            f[5] = ("op", "read");
        }
        if user == "User:payments" && name == "orders" {
            f[5] = ("op", "read");
        }
        let (res, _) = support::send(&app, post_form("/access/acls/new", &f)).await;
        assert_eq!(res.status(), StatusCode::SEE_OTHER);
    }
    let (_, doc) = page(&app, get("/access")).await;
    write("access-secured", &doc);
    let (_, doc) = page(
        &app,
        get("/access/acls/new?type=topic&name=orders&pattern=literal&principal=User%3Aorders&host=%2A&op=write&perm=allow"),
    )
    .await;
    write("access-acl-create", &doc);
    let (_, doc) = page(
        &app,
        get("/access/acls/delete?type=topic&name=orders&pattern=literal&principal=User%3Aorders&host=%2A&op=write&perm=allow"),
    )
    .await;
    write("access-acl-delete", &doc);
    s.w.stop().await;
}
