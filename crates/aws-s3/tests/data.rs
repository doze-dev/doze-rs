//! `doze-aws data` on S3, over a real socket and a real directory: a bucket is measured by kind (current objects, old versions,
//! delete markers, unfinished uploads), pruned by each way it can be, emptied whole with every setting kept, and the blobs and the
//! file give the space back; a restart finds what the live service held; a reset leaves nothing and frees every name.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::too_many_lines
)]

mod support;

use aws_wire::storage::{Emptied, Prune, Resource, Storage, StorageError};
use doze_runtime::Clock;
use support::lab::{DAY_MS, LATER, Lab, lab, lab_at};

const KB64: usize = 64 << 10;
const PRIVATE: &str = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":{"AWS":"arn:aws:iam::111122223333:root"},"Action":"s3:GetObject","Resource":"arn:aws:s3:::pics/*"}]}"#;
const CORS: &str = "<CORSConfiguration><CORSRule><AllowedMethod>GET</AllowedMethod><AllowedOrigin>https://example.com</AllowedOrigin></CORSRule></CORSConfiguration>";

async fn put(lab: &Lab, path: &str, len: usize) {
    let r = lab.call("PUT", path, &[], &"x".repeat(len)).await;
    assert_eq!(r.status, 200, "{path}: {}", r.text());
}

async fn bucket(lab: &Lab, name: &str) -> Resource {
    let usage = lab.s3.usage().await.unwrap();
    usage
        .resources
        .into_iter()
        .find(|r| r.name == name)
        .unwrap()
}

async fn empty(lab: &Lab, name: &str, prune: Prune) -> Emptied {
    lab.s3.empty("us-east-1", name, &prune).await.unwrap()
}

fn blob_files(lab: &Lab) -> usize {
    std::fs::read_dir(lab.dir.join("blobs"))
        .map(|d| d.count())
        .unwrap_or(0)
}

/// A versioned bucket with a policy and CORS, two current objects, an overwritten one, a deleted one, and an unfinished upload.
async fn fill(lab: &Lab) {
    assert_eq!(lab.call("PUT", "/pics", &[], "").await.status, 200);
    let on = "<VersioningConfiguration><Status>Enabled</Status></VersioningConfiguration>";
    assert_eq!(
        lab.call("PUT", "/pics?versioning", &[], on).await.status,
        200
    );
    assert_eq!(
        lab.call("PUT", "/pics?policy", &[], PRIVATE).await.status,
        204
    );
    assert_eq!(lab.call("PUT", "/pics?cors", &[], CORS).await.status, 200);
    put(lab, "/pics/a", KB64).await;
    put(lab, "/pics/a", KB64).await;
    put(lab, "/pics/logs/b", KB64).await;
    put(lab, "/pics/c", KB64).await;
    assert_eq!(lab.call("DELETE", "/pics/c", &[], "").await.status, 204);
    let started = lab.call("POST", "/pics/big?uploads", &[], "").await.text();
    let id = started
        .split("<UploadId>")
        .nth(1)
        .and_then(|t| t.split("</UploadId>").next())
        .unwrap()
        .to_owned();
    put(
        lab,
        &format!("/pics/big?partNumber=1&uploadId={id}"),
        2 * KB64,
    )
    .await;
}

#[tokio::test]
async fn a_bucket_is_measured_by_kind_pruned_each_way_and_emptied_with_its_settings_kept() {
    let lab = lab("data-prunes").await;
    fill(&lab).await;

    let pics = bucket(&lab, "pics").await;
    assert_eq!(pics.items, 2, "current objects: a and logs/b");
    assert!(
        pics.bytes > 6 * KB64 as u64,
        "five versions of 64 KiB and a 128 KiB part: {pics:?}"
    );
    let texts: Vec<&str> = pics.notes.iter().map(|n| n.text.as_str()).collect();
    assert!(
        texts[0].contains("2 old versions and 1 delete marker"),
        "{texts:?}"
    );
    assert!(
        texts[1].contains("1 unfinished multipart upload (1 part)"),
        "{texts:?}"
    );
    assert!(
        texts[2].contains("no lifecycle rule expires old versions"),
        "{texts:?}"
    );
    assert!(pics.notes[0].bytes > 2 * KB64 as u64);
    assert!(pics.notes[1].bytes > 2 * KB64 as u64);
    let usage = lab.s3.usage().await.unwrap();
    assert!(usage.files[0].bytes > 0);

    // A dry run counts and removes nothing.
    let old = pics.notes[0].prune.clone().unwrap();
    let dry = empty(
        &lab,
        "pics",
        Prune {
            dry_run: true,
            ..old.clone()
        },
    )
    .await;
    assert_eq!((dry.items, dry.freed), (3, 0));
    assert_eq!(bucket(&lab, "pics").await, pics);

    // Old versions and delete markers: the current objects stay, the deleted key is gone for good.
    let files = blob_files(&lab);
    let done = empty(&lab, "pics", old).await;
    assert_eq!(done.items, 3);
    assert!(done.freed >= 2 * KB64 as u64, "the blobs went: {done:?}");
    assert_eq!(blob_files(&lab), files - 2);
    assert_eq!(lab.call("GET", "/pics/a", &[], "").await.status, 200);
    let versions = lab.call("GET", "/pics?versions", &[], "").await.text();
    assert!(!versions.contains("<DeleteMarker>"), "{versions}");
    assert_eq!(versions.matches("<Version>").count(), 2, "{versions}");

    // Unfinished uploads.
    let unused = bucket(&lab, "pics").await.notes[0].prune.clone().unwrap();
    assert!(unused.unused);
    assert_eq!(empty(&lab, "pics", unused).await.items, 1);
    let uploads = lab.call("GET", "/pics?uploads", &[], "").await.text();
    assert!(!uploads.contains("<Upload>"), "{uploads}");

    // Before a time: what was written later stays.
    lab.clock.advance(DAY_MS);
    let cut = lab.clock.now_ms();
    put(&lab, "/pics/logs/new", KB64).await;
    put(&lab, "/pics/keep", KB64).await;
    let older = Prune {
        before_ms: Some(cut),
        ..Prune::default()
    };
    assert_eq!(empty(&lab, "pics", older).await.items, 2);
    assert_eq!(lab.call("GET", "/pics/a", &[], "").await.status, 404);
    assert_eq!(lab.call("GET", "/pics/logs/new", &[], "").await.status, 200);

    // Under a prefix.
    let logs = Prune {
        prefix: Some("logs/".into()),
        ..Prune::default()
    };
    assert_eq!(empty(&lab, "pics", logs).await.items, 1);
    assert_eq!(lab.call("GET", "/pics/keep", &[], "").await.status, 200);

    // Whole: no blob is left, the bucket and its settings are.
    let done = empty(&lab, "pics", Prune::default()).await;
    assert_eq!(done.items, 1);
    assert!(done.freed >= KB64 as u64, "{done:?}");
    assert_eq!(blob_files(&lab), 0);
    assert_eq!(lab.call("HEAD", "/pics", &[], "").await.status, 200);
    assert_eq!(lab.call("GET", "/pics?policy", &[], "").await.status, 200);
    assert_eq!(lab.call("GET", "/pics?cors", &[], "").await.status, 200);
    let state = lab.call("GET", "/pics?versioning", &[], "").await.text();
    assert!(state.contains("Enabled"), "{state}");

    // A restart loads exactly what the live service held.
    let (dir, now) = (lab.dir.clone(), lab.clock.now_ms());
    lab.stop().await;
    let lab = lab_at(dir, now).await;
    let pics = bucket(&lab, "pics").await;
    assert_eq!((pics.items, pics.bytes), (0, 0), "{pics:?}");
    assert_eq!(lab.call("GET", "/pics?policy", &[], "").await.status, 200);
    put(&lab, "/pics/again", 10).await;

    // A bucket or region that is not there.
    assert_eq!(
        lab.s3.empty("us-east-1", "nope", &Prune::default()).await,
        Err(StorageError::NotFound)
    );
    assert_eq!(
        lab.s3.empty("eu-west-1", "pics", &Prune::default()).await,
        Err(StorageError::NotFound)
    );
    lab.stop().await;
}

#[tokio::test]
async fn object_lock_is_noted_and_a_local_empty_removes_what_it_holds() {
    let lab = lab("data-lock").await;
    lab.lock_bucket("vault").await;
    put(&lab, "/vault/a", KB64).await;
    let held = lab.retain("/vault/a", &[], "COMPLIANCE", LATER).await;
    assert_eq!(held.status, 200, "{}", held.text());

    let vault = bucket(&lab, "vault").await;
    let note = vault
        .notes
        .iter()
        .find(|n| n.text.contains("Object Lock"))
        .unwrap();
    assert!(note.text.contains("removes them anyway"), "{note:?}");
    assert!(note.bytes >= KB64 as u64);

    let done = empty(&lab, "vault", Prune::default()).await;
    assert_eq!(done.items, 1);
    assert!(done.freed >= KB64 as u64);
    assert_eq!(lab.call("GET", "/vault/a", &[], "").await.status, 404);
    let lock = lab.call("GET", "/vault?object-lock", &[], "").await;
    assert_eq!(lock.status, 200, "the Object Lock configuration stays");
    lab.stop().await;
}

#[tokio::test]
async fn a_reset_removes_every_bucket_and_every_blob_and_frees_the_names() {
    let lab = lab("data-reset").await;
    fill(&lab).await;
    assert_eq!(lab.call("PUT", "/other", &[], "").await.status, 200);
    put(&lab, "/other/k", KB64).await;

    let dry = lab.s3.reset(true).await.unwrap();
    assert_eq!((dry.items, dry.freed), (7, 0), "{dry:?}");
    assert_eq!(lab.call("HEAD", "/other", &[], "").await.status, 200);

    let done = lab.s3.reset(false).await.unwrap();
    assert_eq!(done.items, 7);
    assert!(done.freed >= 6 * KB64 as u64, "{done:?}");
    assert_eq!(blob_files(&lab), 0);
    assert_eq!(lab.call("HEAD", "/pics", &[], "").await.status, 404);

    let (dir, now) = (lab.dir.clone(), lab.clock.now_ms());
    lab.stop().await;
    let lab = lab_at(dir, now).await;
    assert!(lab.s3.usage().await.unwrap().resources.is_empty());
    assert_eq!(
        lab.call("PUT", "/pics", &[], "").await.status,
        200,
        "the name is free at once"
    );
    lab.stop().await;
}
