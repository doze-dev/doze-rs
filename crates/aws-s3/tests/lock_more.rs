//! Object Lock over a real socket, continued: the bucket's default retention, multipart uploads, lifecycle, and the policy status.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::too_many_lines
)]

mod support;
use support::lab::{DAY_MS, LATER, SOON, denied, lab};

#[tokio::test]
async fn the_bucket_default_reaches_new_objects_and_explicit_headers_override_it() {
    let lab = lab("default").await;
    // A bucket made without Object Lock refuses retention, holds and the headers that ask for them.
    assert_eq!(lab.call("PUT", "/plain", &[], "").await.status, 200);
    lab.call(
        "PUT",
        "/plain?versioning",
        &[],
        "<VersioningConfiguration><Status>Enabled</Status></VersioningConfiguration>",
    )
    .await;
    let id = lab.put("/plain/a", &[]).await;
    let _ = id;
    for refused in [
        lab.call("GET", "/plain/a?retention", &[], "").await,
        lab.retain("/plain/a", &[], "GOVERNANCE", SOON).await,
        lab.call(
            "PUT",
            "/plain/a?legal-hold",
            &[],
            "<LegalHold><Status>ON</Status></LegalHold>",
        )
        .await,
        lab.call(
            "PUT",
            "/plain/b",
            &[
                ("x-amz-object-lock-mode", "GOVERNANCE"),
                ("x-amz-object-lock-retain-until-date", SOON),
            ],
            "data",
        )
        .await,
    ] {
        assert_eq!(refused.status, 400, "{}", refused.text());
        assert!(
            refused.text().contains("<Code>InvalidRequest</Code>"),
            "{}",
            refused.text()
        );
    }
    assert_eq!(
        lab.call("GET", "/plain?object-lock", &[], "").await.status,
        404,
        "no configuration yet"
    );

    // Turned on for a versioned bucket, with a default of one day of GOVERNANCE.
    let config = "<ObjectLockConfiguration><ObjectLockEnabled>Enabled</ObjectLockEnabled><Rule><DefaultRetention><Mode>GOVERNANCE</Mode><Days>1</Days></DefaultRetention></Rule></ObjectLockConfiguration>";
    assert_eq!(
        lab.call("PUT", "/plain?object-lock", &[], config)
            .await
            .status,
        200
    );
    let got = lab.call("GET", "/plain?object-lock", &[], "").await;
    assert!(got.text().contains("<Days>1</Days>"), "{}", got.text());
    // Versioning cannot be suspended any more.
    let suspend = lab
        .call(
            "PUT",
            "/plain?versioning",
            &[],
            "<VersioningConfiguration><Status>Suspended</Status></VersioningConfiguration>",
        )
        .await;
    assert_eq!(suspend.status, 409);

    let new = lab.put("/plain/new", &[]).await;
    let head = lab.call("HEAD", "/plain/new", &[], "").await;
    assert_eq!(head.header("x-amz-object-lock-mode"), Some("GOVERNANCE"));
    assert_eq!(
        head.header("x-amz-object-lock-retain-until-date"),
        Some("2023-11-15T22:13:20.000Z")
    );
    assert!(denied(
        &lab.call("DELETE", &format!("/plain/new?versionId={new}"), &[], "")
            .await
    ));
    // The object made before the default has none.
    assert_eq!(
        lab.call("HEAD", "/plain/a", &[], "")
            .await
            .header("x-amz-object-lock-mode"),
        None
    );

    // Explicit headers win over the default; a copy gets its own, not the source's.
    let own = lab
        .put(
            "/plain/own",
            &[
                ("x-amz-object-lock-mode", "COMPLIANCE"),
                ("x-amz-object-lock-retain-until-date", SOON),
                ("x-amz-object-lock-legal-hold", "ON"),
            ],
        )
        .await;
    let head = lab
        .call("HEAD", &format!("/plain/own?versionId={own}"), &[], "")
        .await;
    assert_eq!(head.header("x-amz-object-lock-mode"), Some("COMPLIANCE"));
    assert_eq!(head.header("x-amz-object-lock-legal-hold"), Some("ON"));
    let copy = lab
        .call(
            "PUT",
            "/plain/copy",
            &[("x-amz-copy-source", "/plain/own")],
            "",
        )
        .await;
    assert_eq!(copy.status, 200, "{}", copy.text());
    let head = lab.call("HEAD", "/plain/copy", &[], "").await;
    assert_eq!(
        head.header("x-amz-object-lock-mode"),
        Some("GOVERNANCE"),
        "the default, not COMPLIANCE"
    );
    assert_eq!(head.header("x-amz-object-lock-legal-hold"), None);

    // The headers are checked.
    let lonely = lab
        .call(
            "PUT",
            "/plain/x",
            &[("x-amz-object-lock-mode", "GOVERNANCE")],
            "d",
        )
        .await;
    assert_eq!(lonely.status, 400);
    assert!(
        lonely.text().contains("must both be supplied"),
        "{}",
        lonely.text()
    );
    let past = lab
        .call(
            "PUT",
            "/plain/x",
            &[
                ("x-amz-object-lock-mode", "GOVERNANCE"),
                (
                    "x-amz-object-lock-retain-until-date",
                    "2020-01-01T00:00:00Z",
                ),
            ],
            "d",
        )
        .await;
    assert_eq!(past.status, 400);
    assert!(
        past.text().contains("must be in the future"),
        "{}",
        past.text()
    );
    lab.stop().await;
}

#[tokio::test]
async fn a_multipart_upload_carries_its_lock_to_the_object() {
    let lab = lab("multipart").await;
    lab.lock_bucket("mpb").await;
    let start = lab
        .call(
            "POST",
            "/mpb/big?uploads",
            &[
                ("x-amz-object-lock-mode", "COMPLIANCE"),
                ("x-amz-object-lock-retain-until-date", SOON),
            ],
            "",
        )
        .await;
    assert_eq!(start.status, 200, "{}", start.text());
    let text = start.text();
    let id = text
        .split("<UploadId>")
        .nth(1)
        .and_then(|t| t.split("</UploadId>").next())
        .unwrap()
        .to_owned();
    let part = lab
        .call(
            "PUT",
            &format!("/mpb/big?partNumber=1&uploadId={id}"),
            &[],
            "only part",
        )
        .await;
    assert_eq!(part.status, 200, "{}", part.text());
    let etag = part.header("ETag").unwrap().to_owned();
    let done = lab
        .call(
            "POST",
            &format!("/mpb/big?uploadId={id}"),
            &[],
            &format!("<CompleteMultipartUpload><Part><PartNumber>1</PartNumber><ETag>{etag}</ETag></Part></CompleteMultipartUpload>"),
        )
        .await;
    assert_eq!(done.status, 200, "{}", done.text());
    let version = done
        .text()
        .split("<VersionId>")
        .nth(1)
        .map(|t| t.split('<').next().unwrap().to_owned());
    let head = lab.call("HEAD", "/mpb/big", &[], "").await;
    assert_eq!(head.header("x-amz-object-lock-mode"), Some("COMPLIANCE"));
    let id = head
        .header("x-amz-version-id")
        .map(str::to_owned)
        .or(version)
        .unwrap();
    assert!(denied(
        &lab.call("DELETE", &format!("/mpb/big?versionId={id}"), &[], "")
            .await
    ));
    lab.stop().await;
}

#[tokio::test]
async fn lifecycle_does_not_remove_a_locked_version() {
    let lab = lab("lifecycle").await;
    lab.lock_bucket("lcb").await;
    let old = lab.put("/lcb/a", &[]).await;
    let held = lab.put("/lcb/b", &[]).await;
    lab.put("/lcb/b", &[]).await;
    lab.put("/lcb/a", &[]).await;
    assert_eq!(
        lab.retain("/lcb/a", &[], "COMPLIANCE", LATER).await.status,
        200,
        "the newest a"
    );
    // Lock the older versions of a and b, one by retention and one by hold.
    let r = lab
        .retain(&format!("/lcb/a?versionId={old}"), &[], "GOVERNANCE", LATER)
        .await;
    assert_eq!(r.status, 200, "{}", r.text());
    lab.call(
        "PUT",
        &format!("/lcb/b?legal-hold&versionId={held}"),
        &[],
        "<LegalHold><Status>ON</Status></LegalHold>",
    )
    .await;
    let rules = "<LifecycleConfiguration><Rule><ID>nc</ID><Status>Enabled</Status><Filter><Prefix></Prefix></Filter><NoncurrentVersionExpiration><NoncurrentDays>1</NoncurrentDays></NoncurrentVersionExpiration></Rule></LifecycleConfiguration>";
    assert_eq!(
        lab.call("PUT", "/lcb?lifecycle", &[], rules).await.status,
        200
    );
    lab.clock.advance(3 * DAY_MS);
    let swept = lab.s3.sweep_now("us-east-1").await.unwrap();
    assert_eq!(swept.actions, 0, "both noncurrent versions are locked");
    assert_eq!(
        lab.call("GET", &format!("/lcb/a?versionId={old}"), &[], "")
            .await
            .status,
        200
    );
    assert_eq!(
        lab.call("GET", &format!("/lcb/b?versionId={held}"), &[], "")
            .await
            .status,
        200
    );

    // Once the hold is off the next sweep takes the version.
    lab.call(
        "PUT",
        &format!("/lcb/b?legal-hold&versionId={held}"),
        &[],
        "<LegalHold><Status>OFF</Status></LegalHold>",
    )
    .await;
    assert_eq!(lab.s3.sweep_now("us-east-1").await.unwrap().actions, 1);
    assert_eq!(
        lab.call("GET", &format!("/lcb/b?versionId={held}"), &[], "")
            .await
            .status,
        404
    );
    lab.stop().await;
}

#[tokio::test]
async fn the_policy_status_says_whether_the_stored_policy_is_public() {
    let lab = lab("policy").await;
    assert_eq!(lab.call("PUT", "/ppb", &[], "").await.status, 200);
    let none = lab.call("GET", "/ppb?policyStatus", &[], "").await;
    assert_eq!(none.status, 404);
    assert!(none.text().contains("NoSuchBucketPolicy"));

    let private = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":{"AWS":"arn:aws:iam::111122223333:root"},"Action":"s3:GetObject","Resource":"arn:aws:s3:::ppb/*"}]}"#;
    assert_eq!(
        lab.call("PUT", "/ppb?policy", &[], private).await.status,
        204
    );
    let status = lab.call("GET", "/ppb?policyStatus", &[], "").await;
    assert_eq!(status.status, 200, "{}", status.text());
    assert!(
        status.text().contains("<IsPublic>false</IsPublic>"),
        "{}",
        status.text()
    );

    // A public policy needs the block off, as it does at the put.
    let off = "<PublicAccessBlockConfiguration><BlockPublicAcls>true</BlockPublicAcls><IgnorePublicAcls>true</IgnorePublicAcls><BlockPublicPolicy>false</BlockPublicPolicy><RestrictPublicBuckets>false</RestrictPublicBuckets></PublicAccessBlockConfiguration>";
    assert_eq!(
        lab.call("PUT", "/ppb?publicAccessBlock", &[], off)
            .await
            .status,
        200
    );
    let public = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":"*","Action":"s3:GetObject","Resource":"arn:aws:s3:::ppb/*"}]}"#;
    assert_eq!(
        lab.call("PUT", "/ppb?policy", &[], public).await.status,
        204
    );
    let status = lab.call("GET", "/ppb?policyStatus", &[], "").await;
    assert!(
        status.text().contains("<IsPublic>true</IsPublic>"),
        "{}",
        status.text()
    );
    let conditional = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":{"AWS":"*"},"Action":"s3:GetObject","Resource":"arn:aws:s3:::ppb/*","Condition":{"IpAddress":{"aws:SourceIp":"10.0.0.0/8"}}}]}"#;
    assert_eq!(
        lab.call("PUT", "/ppb?policy", &[], conditional)
            .await
            .status,
        204
    );
    let status = lab.call("GET", "/ppb?policyStatus", &[], "").await;
    assert!(
        status.text().contains("<IsPublic>false</IsPublic>"),
        "{}",
        status.text()
    );
    lab.stop().await;
}
