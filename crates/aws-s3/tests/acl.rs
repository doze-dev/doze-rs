//! Access control lists over a real socket, as S3 has had them since April 2023: disabled on a new bucket (reads answer the owner's
//! full control, writes that grant more are refused), kept and returned once Object Ownership enables them.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::too_many_lines
)]

mod support;
use support::lab::lab;

const PUBLIC_READ: (&str, &str) = ("x-amz-acl", "public-read");
const OWNER_FULL: (&str, &str) = ("x-amz-acl", "bucket-owner-full-control");
const ALL_USERS: &str = "http://acs.amazonaws.com/groups/global/AllUsers";

#[tokio::test]
async fn a_new_bucket_has_acls_disabled_and_reads_answer_the_owner() {
    let lab = lab("acl-enforced").await;
    assert_eq!(lab.call("PUT", "/acb", &[], "").await.status, 200);
    let own = lab.call("GET", "/acb?ownershipControls", &[], "").await;
    assert!(own.text().contains("BucketOwnerEnforced"), "{}", own.text());

    let acl = lab.call("GET", "/acb?acl", &[], "").await;
    assert_eq!(acl.status, 200, "{}", acl.text());
    let text = acl.text();
    assert!(
        text.contains("<Permission>FULL_CONTROL</Permission>"),
        "{text}"
    );
    assert!(
        text.contains(r#"<Grantee xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:type="CanonicalUser">"#),
        "{text}"
    );
    assert!(text.contains("<AccessControlList>"), "{text}");

    // Writing an ACL is refused; the owner's own full control is accepted on an object write.
    let refused = lab.call("PUT", "/acb?acl", &[PUBLIC_READ], "").await;
    assert_eq!(refused.status, 400);
    assert!(refused.text().contains("AccessControlListNotSupported"));
    let private = lab
        .call("PUT", "/acb?acl", &[("x-amz-acl", "private")], "")
        .await;
    assert!(private.text().contains("AccessControlListNotSupported"));
    let put = lab.call("PUT", "/acb/k", &[PUBLIC_READ], "data").await;
    assert_eq!(put.status, 400);
    assert!(put.text().contains("AccessControlListNotSupported"));
    assert_eq!(
        lab.call("PUT", "/acb/k", &[OWNER_FULL], "data")
            .await
            .status,
        200
    );
    let obj = lab.call("GET", "/acb/k?acl", &[], "").await;
    assert_eq!(obj.status, 200);
    assert!(!obj.text().contains(ALL_USERS));
    let put_acl = lab.call("PUT", "/acb/k?acl", &[PUBLIC_READ], "").await;
    assert!(put_acl.text().contains("AccessControlListNotSupported"));
    assert_eq!(
        lab.call("PUT", "/acb/k?acl", &[OWNER_FULL], "")
            .await
            .status,
        200
    );

    // A new bucket may only be made with `private` while ACLs are disabled.
    assert_eq!(
        lab.call("PUT", "/acb2", &[("x-amz-acl", "private")], "")
            .await
            .status,
        200
    );
    let made = lab.call("PUT", "/acb3", &[PUBLIC_READ], "").await;
    assert_eq!(made.status, 400);
    assert!(made.text().contains("InvalidBucketAclWithObjectOwnership"));
    lab.stop().await;
}

#[tokio::test]
async fn with_acls_enabled_they_are_kept_and_public_ones_wait_for_the_block_to_lift() {
    let lab = lab("acl-enabled").await;
    let made = lab
        .call(
            "PUT",
            "/ace",
            &[("x-amz-object-ownership", "ObjectWriter")],
            "",
        )
        .await;
    assert_eq!(made.status, 200, "{}", made.text());

    // A new bucket blocks public ACLs.
    let blocked = lab.call("PUT", "/ace/k", &[PUBLIC_READ], "data").await;
    assert_eq!(blocked.status, 403, "{}", blocked.text());
    assert_eq!(
        lab.call("DELETE", "/ace?publicAccessBlock", &[], "")
            .await
            .status,
        204
    );

    assert_eq!(
        lab.call("PUT", "/ace/k", &[PUBLIC_READ], "data")
            .await
            .status,
        200
    );
    let acl = lab.call("GET", "/ace/k?acl", &[], "").await.text();
    assert!(acl.contains(&format!("<URI>{ALL_USERS}</URI>")), "{acl}");
    assert!(acl.contains(r#"xsi:type="Group""#), "{acl}");
    assert!(acl.contains("<Permission>READ</Permission>"), "{acl}");

    // A body, as an SDK sends it, replaces the object's ACL.
    let body = r#"<AccessControlPolicy xmlns="http://s3.amazonaws.com/doc/2006-03-01/"><AccessControlList><Grant><Grantee xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:type="Group"><URI>http://acs.amazonaws.com/groups/s3/LogDelivery</URI></Grantee><Permission>WRITE</Permission></Grant></AccessControlList></AccessControlPolicy>"#;
    let put = lab.call("PUT", "/ace/k?acl", &[], body).await;
    assert_eq!(put.status, 200, "{}", put.text());
    let acl = lab.call("GET", "/ace/k?acl", &[], "").await.text();
    assert!(
        acl.contains("LogDelivery") && !acl.contains(ALL_USERS),
        "{acl}"
    );

    // The bucket's own ACL is kept too, and turning ACLs off is refused while it grants anyone else.
    let canned = lab
        .call("PUT", "/ace?acl", &[("x-amz-acl", "public-read")], "")
        .await;
    assert_eq!(canned.status, 200, "{}", canned.text());
    assert!(
        lab.call("GET", "/ace?acl", &[], "")
            .await
            .text()
            .contains(ALL_USERS)
    );
    let enforce = r"<OwnershipControls><Rule><ObjectOwnership>BucketOwnerEnforced</ObjectOwnership></Rule></OwnershipControls>";
    let refused = lab
        .call("PUT", "/ace?ownershipControls", &[], enforce)
        .await;
    assert_eq!(refused.status, 400, "{}", refused.text());
    assert!(
        refused
            .text()
            .contains("InvalidBucketAclWithObjectOwnership")
    );
    assert_eq!(
        lab.call("PUT", "/ace?acl", &[("x-amz-acl", "private")], "")
            .await
            .status,
        200
    );
    assert_eq!(
        lab.call("PUT", "/ace?ownershipControls", &[], enforce)
            .await
            .status,
        200
    );
    // ACLs disabled again: the object's grants are kept but no longer answered.
    let acl = lab.call("GET", "/ace/k?acl", &[], "").await.text();
    assert!(!acl.contains("LogDelivery"), "{acl}");

    // Deleting the ownership controls enables ACLs (as ObjectWriter) and the controls read as not found.
    assert_eq!(
        lab.call("DELETE", "/ace?ownershipControls", &[], "")
            .await
            .status,
        204
    );
    let gone = lab.call("GET", "/ace?ownershipControls", &[], "").await;
    assert!(
        gone.text().contains("OwnershipControlsNotFoundError"),
        "{}",
        gone.text()
    );
    let acl = lab.call("GET", "/ace/k?acl", &[], "").await.text();
    assert!(acl.contains("LogDelivery"), "{acl}");
    lab.stop().await;
}

#[tokio::test]
async fn an_acl_request_is_checked() {
    let lab = lab("acl-checks").await;
    lab.call(
        "PUT",
        "/acc",
        &[("x-amz-object-ownership", "BucketOwnerPreferred")],
        "",
    )
    .await;
    lab.call("PUT", "/acc/k", &[], "data").await;
    let both = lab
        .call(
            "PUT",
            "/acc/k?acl",
            &[PUBLIC_READ, ("x-amz-grant-read", "id=\"abc\"")],
            "",
        )
        .await;
    assert_eq!(both.status, 400);
    assert!(both.text().contains("InvalidRequest"));
    let none = lab.call("PUT", "/acc/k?acl", &[], "").await;
    assert_eq!(none.status, 400);
    assert!(
        none.text().contains("MissingSecurityHeader"),
        "{}",
        none.text()
    );
    let email = lab
        .call(
            "PUT",
            "/acc/k?acl",
            &[("x-amz-grant-read", "emailAddress=\"a@b.c\"")],
            "",
        )
        .await;
    assert!(email.text().contains("UnresolvableGrantByEmailAddress"));
    let grant = lab
        .call(
            "PUT",
            "/acc/k?acl",
            &[("x-amz-grant-read", "id=\"abc\"")],
            "",
        )
        .await;
    assert_eq!(grant.status, 200, "{}", grant.text());
    assert!(
        lab.call("GET", "/acc/k?acl", &[], "")
            .await
            .text()
            .contains("<ID>abc</ID>")
    );
    let missing = lab.call("GET", "/acc/nope?acl", &[], "").await;
    assert_eq!(missing.status, 404);
    lab.stop().await;
}
