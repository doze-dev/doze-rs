//! The four boards of the Access page, rendered from the "larder" fixtures with no broker: written to
//! `target/board-diff/<id>.html` for `tools/board-diff.ts`, and checked here against what the boards say.
#![allow(
    clippy::too_many_lines,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::path::PathBuf;

use kafka_console::base::Base;
use kafka_console::boards_access::{IDS, render};

fn out_dir() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest.parent().unwrap().parent().unwrap();
    let target =
        std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| root.join("target"), PathBuf::from);
    target.join("board-diff")
}

/// Every text of `want` appears in `doc`, in this order.
fn in_order(id: &str, doc: &str, want: &[&str]) {
    let mut from = 0;
    for text in want {
        let at = doc[from..]
            .find(text)
            .unwrap_or_else(|| panic!("{id}: {text:?} is not on the page after byte {from}"));
        from += at + text.len();
    }
}

fn page(id: &str) -> String {
    render(id, &Base::root()).unwrap_or_else(|| panic!("no render for {id}"))
}

/// CON-554: the boards render, whole, and are written for the tool.
#[test]
fn every_board_of_the_slice_renders_and_is_written_for_the_tool() {
    let dir = out_dir();
    std::fs::create_dir_all(&dir).unwrap();
    assert_eq!(IDS.len(), 4);
    for id in IDS {
        let doc = page(id);
        assert!(doc.starts_with("<!DOCTYPE html>"), "{id}");
        std::fs::write(dir.join(format!("{id}.html")), &doc).unwrap();
    }
    assert!(render("settings-home", &Base::root()).is_none());
}

/// CON-555: the open broker's page has the parts of its board in order.
#[test]
fn the_open_page_has_the_parts_of_its_board_in_order() {
    in_order(
        "access-open",
        &page("access-open"),
        &[
            "<h1>Access</h1>",
            "<span class=\"st idle\">Open</span>",
            "Anyone who can reach <b>127.0.0.1:9092</b> may read, write and change anything",
            "Read again",
            "<h2>Authentication</h2>",
            "Off",
            "Mechanisms",
            "None",
            "<span class=\"mono\">--sasl-mechanisms</span>",
            "Users",
            "0 from flags, 0 stored",
            "names only; a password or key is never shown",
            "Caller",
            "<h2>Authorizer</h2>",
            "Allows by default",
            "--acl-default allow",
            "Super users",
            "a super user is never refused, DENY included",
            "<h2>Encryption</h2>",
            "Plain TCP between client and broker.",
            "Certificate",
            "Listener",
            "loopback: this machine only",
            "<h2>ACLs</h2>",
            "Create ACL",
            "Resource",
            "Pattern",
            "Principal",
            "Host",
            "Operation",
            "Permission",
            "<h3>No ACLs</h3>",
            "<h2>The console</h2>",
            "User:__console",
            "Super user",
            "Anyone who can reach it",
            "127.0.0.1:9093",
            "Allowed hosts",
            "Same origin only",
            "<h2>What the broker says at start</h2>",
            "Console served beyond loopback with SASL or TLS on",
            "SASL without TLS beyond loopback",
            "SASL on with ",
            "not the case",
        ],
    );
}

/// CON-556: the secured broker's page has the parts of its board in order, with its eight ACLs.
#[test]
fn the_secured_page_has_the_parts_of_its_board_in_order() {
    let doc = page("access-secured");
    in_order(
        "access-secured",
        &doc,
        &[
            "<span class=\"st ok\">Sign-in required</span>",
            "Clients sign in with SCRAM-SHA-256 or SCRAM-SHA-512 over TLS",
            "<h2>Authentication</h2>",
            "SCRAM-SHA-256, SCRAM-SHA-512",
            "PLAIN is off: a stored SCRAM user cannot use it",
            "1 from flags, 3 stored",
            "A failed login",
            "waits 100 ms",
            "<h2>Authorizer</h2>",
            "Denies by default",
            "--acl-default deny",
            "7 allow, 1 deny",
            "User:admin",
            "<h2>Encryption</h2>",
            "TLS on",
            "~/larder/tls/broker.pem",
            "the chain, leaf first",
            "Private key",
            "its contents are printed nowhere",
            "<h2>Users</h2>",
            "admin",
            "Flags",
            "a super user",
            "payments",
            "Stored",
            "<h2>ACLs</h2>",
            "payments.",
            "Prefixed",
            "IdempotentWrite",
            "report-job",
            "Deny",
            "<h2>The console</h2>",
            "<h2>What the broker says at start</h2>",
        ],
    );
    assert_eq!(
        doc.matches("data-op=\"DeleteAcls\"").count(),
        8,
        "a Delete on each of the eight"
    );
    assert!(
        doc.contains("<th scope=\"col\">Iterations</th>"),
        "the iteration count is a public parameter, and the board draws it"
    );
}

/// CON-557: the two dialogs are the boards', over the secured page.
#[test]
fn the_two_dialogs_are_the_boards_over_their_parent() {
    in_order(
        "access-acl-create",
        &page("access-acl-create"),
        &[
            "<h1>Access</h1>",
            "role=\"dialog\"",
            "Create an ACL",
            "Resource type",
            "Resource name",
            "orders",
            "Name matches",
            "Literal",
            "Prefixed",
            "Principal",
            "User:orders",
            "Host",
            "Operation",
            "Permission",
            "Allow",
            "Deny",
            "Kafka says",
            "Allow implies",
            "Host</b>",
            "The call, asked as User:__console",
            "CreateAcls",
            "Cancel",
            "Create ACL",
        ],
    );
    in_order(
        "access-acl-delete",
        &page("access-acl-delete"),
        &[
            "<h1>Access</h1>",
            "role=\"dialog\"",
            "Delete this ACL?",
            "Resource",
            "Topic",
            "orders",
            "literal",
            "Principal",
            "User:orders",
            "Host",
            "Operation",
            "Write",
            "Allow",
            "It takes effect on the next request.",
            "TOPIC_AUTHORIZATION_FAILED",
            "(29)",
            "Only the exact ACL is removed.",
            "The call, asked as User:__console",
            "DeleteAcls",
            "filters[0]",
            "Cancel",
            "Delete ACL",
        ],
    );
}
