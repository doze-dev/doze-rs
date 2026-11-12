//! The Access page and its two dialogs over a real broker in this process: SCRAM on, `--acl-default deny`, a super user, a user
//! from flags and a stored one. What the page lists is what the broker lists; an ACL made and deleted through the pages is seen
//! through DescribeAcls; each refusal is Kafka's own; hostile names are escaped; a GET changes nothing; and no secret of a
//! credential is anywhere.
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

use axum::http::{StatusCode, header};
use kafka_auth::Principal;
use kafka_broker::Caller;
use kafka_wire::messages::ApiKey;
use support::{get, send};
use support_access::{acls, form, new_acl, router_with, secured, start};
use support_write::{enc, fragment_get, page, post_form};

/// CON-558: the page lists what the broker holds, by name, and says what protects the console.
#[tokio::test]
async fn the_page_says_what_a_secured_broker_holds_by_name_only() {
    let s = secured("acc-page", false).await;
    let app = s.router();
    let (status, doc) = page(&app, get("/access")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        doc.contains("<span class=\"st ok\">Sign-in required</span>"),
        "{doc:.2000}"
    );
    assert!(
        doc.contains("SCRAM-SHA-256, SCRAM-SHA-512") && doc.contains("Denies by default"),
        "{doc:.3000}"
    );
    assert!(
        doc.contains("1 from flags, 1 stored"),
        "the broker holds admin from flags and payments stored"
    );
    assert!(doc.contains("waits 100 ms"));
    // The users table: admin is from flags and a super user, payments is stored.
    assert!(doc.contains("<td class=\"name mono\">admin</td><td><span class=\"pill\">Flags</span><span class=\"sub\">a super user</span></td>"), "{doc}");
    assert!(doc.contains("<td class=\"name mono\">payments</td><td><span class=\"pill\">Stored</span></td><td class=\"mono\">SCRAM-SHA-256</td>"), "{doc}");
    // The super users are the operator's: the console's own is on its own line.
    assert!(doc.contains("<div class=\"mono\">User:admin</div>"));
    assert!(doc.contains("User:__console") && doc.contains("Anyone who can reach it"));
    assert!(doc.contains("No ACLs"), "{doc}");
    assert!(
        !doc.contains("applies"),
        "no start-up warning holds: loopback, SCRAM and deny"
    );
    s.w.stop().await;
}

/// CON-559: an ACL is made and deleted through the pages and seen through DescribeAcls.
#[tokio::test]
async fn an_acl_made_and_deleted_through_the_pages_is_seen_through_describeacls() {
    let s = secured("acc-crud", false).await;
    let app = s.router();
    assert!(acls(&s.w).await.is_empty());
    // The dialog is a page of its own, over the Access page.
    let (status, dialog) = page(&app, get("/access/acls/new")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        dialog.contains("<h1>Access</h1>") && dialog.contains("Create an ACL"),
        "{dialog:.500}"
    );
    // Create.
    let (res, _) = send(&app, form(&new_acl("orders", "User:orders"))).await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert_eq!(res.headers()[header::LOCATION], "/access");
    assert_eq!(
        acls(&s.w).await,
        [(
            2,
            "orders".to_owned(),
            3,
            "User:orders".to_owned(),
            "*".to_owned(),
            4,
            3
        )]
    );
    // The page lists it, and its Delete names it exactly.
    let (_, doc) = page(&app, get("/access")).await;
    assert!(doc.contains("<span class=\"mono\">orders</span></td><td>Literal</td><td class=\"mono\">User:orders</td>"), "{doc}");
    assert!(
        doc.contains("1 allow, 0 deny") || doc.contains("1</div><div class=\"help\""),
        "{doc}"
    );
    let link = doc
        .split("href=\"")
        .map(|s| s.split('"').next().unwrap())
        .find(|h| h.starts_with("/access/acls/delete?"))
        .unwrap()
        .replace("&amp;", "&");
    // The delete dialog: what the broker would decide afterwards, by Kafka's own rules (nothing else covers orders).
    let (status, dlg) = page(&app, get(&link)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        dlg.contains("Delete this ACL?")
            && dlg.contains("is then refused: its next Produce is answered"),
        "{dlg}"
    );
    assert!(dlg.contains("TOPIC_AUTHORIZATION_FAILED") && dlg.contains("(29)"));
    // A second ACL for the same topic: the default stays off for it, and the sentence says so.
    let (res, _) = send(&app, form(&new_acl("orders", "User:payments"))).await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    let (_, dlg) = page(&app, get(&link)).await;
    assert!(
        dlg.contains("is also covered by 1 other ACL, so the default stays off for it"),
        "{dlg}"
    );
    // Delete the first, by the dialog's own form fields.
    let fields: Vec<(&str, &str)> = vec![
        ("type", "topic"),
        ("name", "orders"),
        ("pattern", "literal"),
        ("principal", "User:orders"),
        ("host", "*"),
        ("op", "write"),
        ("perm", "allow"),
    ];
    let (res, _) = send(&app, post_form("/access/acls/delete", &fields)).await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        acls(&s.w).await,
        [(
            2,
            "orders".to_owned(),
            3,
            "User:payments".to_owned(),
            "*".to_owned(),
            4,
            3
        )],
        "only the exact ACL went"
    );
    // Deleting it again says it is gone.
    let (status, gone) = page(&app, post_form("/access/acls/delete", &fields)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(gone.contains("This ACL is not there any more"), "{gone}");
    s.w.stop().await;
}

/// CON-560: each refusal is Kafka's own words with its code; a missing field is said before anything is asked.
#[tokio::test]
async fn each_refusal_is_kafkas_own_and_a_missing_field_asks_the_broker_nothing() {
    let s = secured("acc-refuse", false).await;
    let (spy, log) = s.w.backend.recording();
    let app = router_with(s.w.facts.clone(), start(false), spy);
    // A principal with no colon.
    let (status, doc) = page(&app, form(&new_acl("orders", "orders"))).await;
    assert_eq!(status, StatusCode::OK);
    assert!(doc.contains("Refused · INVALID_REQUEST (42)"), "{doc}");
    assert!(
        doc.contains("Could not parse principal from `orders`"),
        "{doc}"
    );
    assert!(doc.contains("value=\"orders\""), "what was typed stays");
    // The CLUSTER resource has one name.
    let mut cluster = new_acl("x", "User:a");
    cluster[0].1 = "cluster".to_owned();
    let (_, doc) = page(&app, form(&cluster)).await;
    assert!(
        doc.contains("The only valid name for the CLUSTER resource is kafka-cluster"),
        "{doc}"
    );
    assert!(acls(&s.w).await.is_empty(), "nothing was made");
    // A missing name is the console's own sentence, and asks the broker nothing.
    log.lock().unwrap().clear();
    let (status, doc) = page(&app, form(&new_acl("", "User:a"))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        doc.contains("Type the resource&#39;s name.") || doc.contains("Type the resource's name."),
        "{doc}"
    );
    assert!(!log.lock().unwrap().contains(&ApiKey::CreateAcls));
    // A caller that may not alter the cluster is refused as a client would be (31), for each of the two writes.
    let nobody = Caller::new(
        Principal::user("nobody"),
        Some("127.0.0.1".parse().unwrap()),
    );
    let app = router_with(
        s.w.facts.clone(),
        start(false),
        s.w.backend.acting_as(nobody),
    );
    let (status, doc) = page(&app, form(&new_acl("orders", "User:orders"))).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        doc.contains("Refused · CLUSTER_AUTHORIZATION_FAILED (31)")
            && doc.contains("Cluster authorization failed."),
        "{doc}"
    );
    let fields = [
        ("type", "topic"),
        ("name", "orders"),
        ("principal", "User:orders"),
        ("op", "write"),
    ];
    let (_, doc) = page(&app, post_form("/access/acls/delete", &fields)).await;
    assert!(
        doc.contains("Refused · CLUSTER_AUTHORIZATION_FAILED (31)"),
        "{doc}"
    );
    // The page itself, for such a caller, is Kafka's refusal and not a table.
    let (status, doc) = page(&app, get("/access")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        doc.contains("Refused · CLUSTER_AUTHORIZATION_FAILED (31)"),
        "{doc:.3000}"
    );
    s.w.stop().await;
}

/// CON-561: hostile names are escaped everywhere they are shown, and cannot inject a header or a line of the call.
#[tokio::test]
async fn hostile_names_are_escaped_and_inject_nothing() {
    let s = secured("acc-hostile", false).await;
    let app = s.router();
    let name = "<script>alert(1)</script>";
    let who = "User:\"><img src=x onerror=alert(2)>";
    let (res, _) = send(&app, form(&new_acl(name, who))).await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    let (_, doc) = page(&app, get("/access")).await;
    for bad in ["<script>alert(1)", "<img src=x"] {
        assert!(!doc.contains(bad), "{bad} unescaped: {doc}");
    }
    assert!(doc.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
    let link = doc
        .split("href=\"")
        .map(|s| s.split('"').next().unwrap())
        .find(|h| h.starts_with("/access/acls/delete?"))
        .unwrap()
        .replace("&amp;", "&");
    let (status, dlg) = page(&app, get(&link)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        !dlg.contains("<script>alert(1)") && !dlg.contains("<img src=x"),
        "{dlg}"
    );
    assert!(
        dlg.contains("is then refused"),
        "the ACL was found again from its own address"
    );
    // A name that carries a line break and a header: the call is drawn on one line each, and no header appears.
    let evil = "a\r\nSet-Cookie: pwned=1\r\n\r\n<b>x</b>";
    let (res, text) = send(
        &app,
        get(&format!(
            "/access/acls/delete?type=topic&name={}&principal=User%3Ax&op=write",
            enc(evil)
        )),
    )
    .await;
    assert!(res.headers().get(header::SET_COOKIE).is_none());
    assert!(
        !text.contains("<b>x</b>") && text.contains("\\r\\nSet-Cookie"),
        "{text}"
    );
    let (res, _) = send(&app, form(&new_acl(evil, "User:a"))).await;
    assert!(res.headers().get(header::SET_COOKIE).is_none());
    assert_eq!(res.headers()[header::LOCATION], "/access");
    s.w.stop().await;
}

/// CON-562: a GET never asks the broker to change anything.
#[tokio::test]
async fn a_get_of_either_page_or_dialog_changes_nothing() {
    let s = secured("acc-get", false).await;
    let (spy, log) = s.w.backend.recording();
    let app = router_with(s.w.facts.clone(), start(false), spy);
    send(&app, form(&new_acl("orders", "User:orders"))).await;
    log.lock().unwrap().clear();
    let before = acls(&s.w).await;
    for path in [
        "/access",
        "/access?state=secured",
        "/access/acls/new",
        "/access/acls/new?type=group&name=g&principal=User%3Aa",
        "/access/acls/delete?type=topic&name=orders&pattern=literal&principal=User%3Aorders&host=%2A&op=write&perm=allow",
        "/access/acls/delete",
        "/settings",
        "/settings/config/edit?key=log.retention.ms",
        "/settings/config/edit?key=num.partitions",
    ] {
        let (status, _) = page(&app, get(path)).await;
        assert!(status.as_u16() < 500, "{path} answered {status}");
        let (status, _) = page(&app, fragment_get(path)).await;
        assert!(
            status.as_u16() < 500,
            "{path} as a fragment answered {status}"
        );
    }
    let calls = log.lock().unwrap().clone();
    assert!(!calls.is_empty());
    let reads = [
        ApiKey::DescribeAcls,
        ApiKey::DescribeUserScramCredentials,
        ApiKey::DescribeConfigs,
        ApiKey::Metadata,
    ];
    for c in &calls {
        assert!(reads.contains(c), "a GET called {c:?}, which is not a read");
    }
    assert_eq!(acls(&s.w).await, before);
    s.w.stop().await;
}
