//! Object Lock over a real socket, with a clock the test moves: retention and legal holds block exactly the deletes AWS blocks.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::too_many_lines
)]

mod support;
use support::lab::{BYPASS, DAY_MS, LATER, SOON, denied, lab};

#[tokio::test]
async fn retention_blocks_a_delete_that_names_the_version_and_governance_gives_way_to_bypass() {
    let lab = lab("retention").await;
    lab.lock_bucket("lkb").await;
    let id = lab.put("/lkb/a", &[]).await;
    assert!(!id.is_empty(), "an Object Lock bucket is versioned");

    // Nothing is retained yet.
    let none = lab.call("GET", "/lkb/a?retention", &[], "").await;
    assert_eq!(none.status, 404);
    assert!(none.text().contains("NoSuchObjectLockConfiguration"));

    let put = lab.retain("/lkb/a", &[], "GOVERNANCE", SOON).await;
    assert_eq!(put.status, 200, "{}", put.text());
    let got = lab.call("GET", "/lkb/a?retention", &[], "").await;
    assert_eq!(got.status, 200);
    assert!(
        got.text().contains("<Mode>GOVERNANCE</Mode>"),
        "{}",
        got.text()
    );
    assert!(
        got.text()
            .contains("<RetainUntilDate>2023-11-20T00:00:00.000Z</RetainUntilDate>"),
        "{}",
        got.text()
    );
    let head = lab.call("HEAD", "/lkb/a", &[], "").await;
    assert_eq!(head.header("x-amz-object-lock-mode"), Some("GOVERNANCE"));
    assert_eq!(
        head.header("x-amz-object-lock-retain-until-date"),
        Some("2023-11-20T00:00:00.000Z")
    );

    // A delete without a version only adds a marker; one that names the version is refused.
    let marker = lab.call("DELETE", "/lkb/a", &[], "").await;
    assert_eq!(marker.status, 204);
    assert_eq!(marker.header("x-amz-delete-marker"), Some("true"));
    let path = format!("/lkb/a?versionId={id}");
    let refused = lab.call("DELETE", &path, &[], "").await;
    assert!(denied(&refused), "{}", refused.text());
    assert!(
        refused
            .text()
            .contains("Access Denied because object protected by object lock.")
    );
    // The version is still there.
    assert_eq!(lab.call("GET", &path, &[], "").await.status, 200);

    // Shortening needs the bypass; extending does not.
    let shorter = lab
        .retain(&path, &[], "GOVERNANCE", "2023-11-17T00:00:00Z")
        .await;
    assert!(denied(&shorter), "{}", shorter.text());
    assert_eq!(
        lab.retain(&path, &[], "GOVERNANCE", LATER).await.status,
        200
    );
    assert_eq!(
        lab.retain(&path, &[BYPASS], "GOVERNANCE", "2023-11-17T00:00:00Z")
            .await
            .status,
        200
    );

    // With the bypass the version can go.
    assert_eq!(lab.call("DELETE", &path, &[BYPASS], "").await.status, 204);
    assert_eq!(lab.call("GET", &path, &[], "").await.status, 404);
    lab.stop().await;
}

#[tokio::test]
async fn compliance_cannot_be_bypassed_shortened_or_weakened_until_it_runs_out() {
    let lab = lab("compliance").await;
    lab.lock_bucket("clb").await;
    let id = lab.put("/clb/a", &[]).await;
    let path = format!("/clb/a?versionId={id}");
    assert_eq!(
        lab.retain("/clb/a", &[], "COMPLIANCE", SOON).await.status,
        200
    );

    assert!(denied(&lab.call("DELETE", &path, &[BYPASS], "").await));
    assert!(denied(
        &lab.retain("/clb/a", &[BYPASS], "COMPLIANCE", "2023-11-16T00:00:00Z")
            .await
    ));
    assert!(denied(
        &lab.retain("/clb/a", &[BYPASS], "GOVERNANCE", LATER).await
    ));
    // Extended, it still stands.
    assert_eq!(
        lab.retain("/clb/a", &[], "COMPLIANCE", LATER).await.status,
        200
    );

    // DeleteObjects reports the key it cannot delete and goes on with the rest.
    let free = lab.put("/clb/free", &[]).await;
    let body = format!(
        "<Delete><Object><Key>a</Key><VersionId>{id}</VersionId></Object><Object><Key>free</Key><VersionId>{free}</VersionId></Object></Delete>"
    );
    let batch = lab.call("POST", "/clb?delete", &[BYPASS], &body).await;
    assert_eq!(batch.status, 200, "{}", batch.text());
    let text = batch.text();
    assert!(
        text.contains("<Error><Key>a</Key>") && text.contains("<Code>AccessDenied</Code>"),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "<Deleted><Key>free</Key><VersionId>{free}</VersionId>"
        )),
        "{text}"
    );

    // Nine days on it has run out (LATER is ten days from the start), and the version goes.
    lab.clock.advance(9 * DAY_MS);
    assert!(denied(&lab.call("DELETE", &path, &[], "").await));
    lab.clock.advance(2 * DAY_MS);
    assert_eq!(lab.call("DELETE", &path, &[], "").await.status, 204);
    lab.stop().await;
}

#[tokio::test]
async fn a_legal_hold_blocks_until_it_is_switched_off_and_is_not_bypassed() {
    let lab = lab("hold").await;
    lab.lock_bucket("lhb").await;
    let id = lab.put("/lhb/a", &[]).await;
    let path = format!("/lhb/a?versionId={id}");
    let unset = lab.call("GET", "/lhb/a?legal-hold", &[], "").await;
    assert_eq!(unset.status, 404);

    let on = lab
        .call(
            "PUT",
            "/lhb/a?legal-hold",
            &[],
            "<LegalHold><Status>ON</Status></LegalHold>",
        )
        .await;
    assert_eq!(on.status, 200, "{}", on.text());
    let got = lab.call("GET", "/lhb/a?legal-hold", &[], "").await;
    assert!(got.text().contains("<Status>ON</Status>"), "{}", got.text());
    assert_eq!(
        lab.call("HEAD", "/lhb/a", &[], "")
            .await
            .header("x-amz-object-lock-legal-hold"),
        Some("ON")
    );
    assert!(denied(&lab.call("DELETE", &path, &[BYPASS], "").await));

    let off = lab
        .call(
            "PUT",
            "/lhb/a?legal-hold",
            &[],
            "<LegalHold><Status>OFF</Status></LegalHold>",
        )
        .await;
    assert_eq!(off.status, 200);
    assert!(
        lab.call("GET", "/lhb/a?legal-hold", &[], "")
            .await
            .text()
            .contains("<Status>OFF</Status>")
    );
    assert_eq!(lab.call("DELETE", &path, &[], "").await.status, 204);
    lab.stop().await;
}
