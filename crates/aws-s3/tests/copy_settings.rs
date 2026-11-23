//! What a copy does with the settings its request names, over a real socket: the storage class and the encryption are the request's
//! whichever way the metadata goes (copied from the source or replaced), and a copy that names no encryption gets the destination
//! bucket's default, not the source's (the model's words for `StorageClass` and `ServerSideEncryption` on `CopyObject`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;
use support::lab::{Lab, lab};

const KMS: (&str, &str) = ("x-amz-server-side-encryption", "aws:kms");
const KEY: (&str, &str) = (
    "x-amz-server-side-encryption-aws-kms-key-id",
    "alias/larder-app",
);

async fn copy(lab: &Lab, to: &str, from: &str, extra: &[(&str, &str)]) {
    let mut headers = vec![("x-amz-copy-source", from)];
    headers.extend_from_slice(extra);
    let r = lab.call("PUT", to, &headers, "").await;
    assert_eq!(r.status, 200, "{to}: {}", r.text());
}

async fn head(lab: &Lab, path: &str) -> support::Reply {
    let r = lab.call("HEAD", path, &[], "").await;
    assert_eq!(r.status, 200, "{path}");
    r
}

#[tokio::test]
async fn a_storage_class_and_a_key_are_the_requests_whichever_directive_it_gives() {
    let lab = lab("copy-settings").await;
    for b in ["/src", "/dst"] {
        assert_eq!(lab.call("PUT", b, &[], "").await.status, 200);
    }
    lab.put("/src/k", &[("x-amz-meta-source", "catalog")]).await;

    // The default directive keeps the source's metadata and still takes the class the request names.
    copy(
        &lab,
        "/dst/class",
        "/src/k",
        &[("x-amz-storage-class", "STANDARD_IA")],
    )
    .await;
    let r = head(&lab, "/dst/class").await;
    assert_eq!(r.header("x-amz-storage-class"), Some("STANDARD_IA"));
    assert_eq!(r.header("x-amz-meta-source"), Some("catalog"));

    // So does a key: the copy is encrypted as the request says, with the key it names.
    copy(&lab, "/dst/kms", "/src/k", &[KMS, KEY]).await;
    let r = head(&lab, "/dst/kms").await;
    assert_eq!(r.header(KMS.0), Some("aws:kms"));
    assert_eq!(r.header(KEY.0), Some("alias/larder-app"));
    assert_eq!(r.header("x-amz-meta-source"), Some("catalog"));

    // A copy that names none is encrypted as the destination bucket is, not as the source was.
    lab.put("/src/kms", &[KMS, KEY]).await;
    copy(&lab, "/dst/plain", "/src/kms", &[]).await;
    let r = head(&lab, "/dst/plain").await;
    assert_eq!(r.header(KMS.0), Some("AES256"));
    assert_eq!(r.header(KEY.0), None);

    // Naming AES256 over a destination whose default is KMS leaves no key on the copy.
    let kms_default = "<ServerSideEncryptionConfiguration><Rule><ApplyServerSideEncryptionByDefault><SSEAlgorithm>aws:kms</SSEAlgorithm><KMSMasterKeyID>alias/larder-app</KMSMasterKeyID></ApplyServerSideEncryptionByDefault></Rule></ServerSideEncryptionConfiguration>";
    assert_eq!(
        lab.call("PUT", "/dst?encryption", &[], kms_default)
            .await
            .status,
        200
    );
    copy(&lab, "/dst/inherits", "/src/k", &[]).await;
    let r = head(&lab, "/dst/inherits").await;
    assert_eq!(r.header(KMS.0), Some("aws:kms"));
    assert_eq!(r.header(KEY.0), Some("alias/larder-app"));
    copy(
        &lab,
        "/dst/s3",
        "/src/k",
        &[("x-amz-server-side-encryption", "AES256")],
    )
    .await;
    let r = head(&lab, "/dst/s3").await;
    assert_eq!(r.header(KMS.0), Some("AES256"));
    assert_eq!(r.header(KEY.0), None);

    // The replace directive takes the request's metadata, and the class with it.
    copy(
        &lab,
        "/dst/replaced",
        "/src/k",
        &[
            ("x-amz-metadata-directive", "REPLACE"),
            ("x-amz-meta-owner", "web"),
            ("x-amz-storage-class", "ONEZONE_IA"),
        ],
    )
    .await;
    let r = head(&lab, "/dst/replaced").await;
    assert_eq!(r.header("x-amz-meta-source"), None);
    assert_eq!(r.header("x-amz-meta-owner"), Some("web"));
    assert_eq!(r.header("x-amz-storage-class"), Some("ONEZONE_IA"));
    lab.stop().await;
}
