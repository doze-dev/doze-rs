//! S3 from Go's `s3/meta.bolt` (internal/s3store in the Go doze-aws): bucket `buckets` (name → bucket JSON),
//! `ver:<bucket>` (`key \0 be64(^seq)` → version JSON, newest first for a key), `up:<bucket>` (multipart uploads), and
//! each version's bytes in the file its `blob` names, under the store's folder.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use super::{Replayed, enc, xml_all};
use crate::client::Client;

#[derive(Deserialize)]
struct GoBucket {
    name: String,
    #[serde(default)]
    versioning: String,
    #[serde(default)]
    tags: BTreeMap<String, String>,
    #[serde(default)]
    cors: String,
    #[serde(default)]
    lifecycle: String,
    #[serde(default)]
    website: String,
    #[serde(default)]
    policy: String,
    #[serde(default)]
    encryption: String,
    #[serde(default)]
    notification: String,
    #[serde(default)]
    replication: String,
}

#[derive(Deserialize)]
struct GoVersion {
    key: String,
    #[serde(default)]
    blob: String,
    #[serde(default)]
    content_type: String,
    #[serde(default)]
    meta: BTreeMap<String, String>,
    #[serde(default)]
    headers: BTreeMap<String, String>,
    #[serde(default)]
    tags: BTreeMap<String, String>,
    #[serde(default)]
    storage_class: String,
    #[serde(default)]
    delete_marker: bool,
    #[serde(default)]
    retain_mode: String,
    #[serde(default)]
    legal_hold: bool,
}

fn ok(what: &str, r: &crate::client::Reply) -> Result<(), String> {
    if (200..300).contains(&r.status) {
        Ok(())
    } else {
        Err(format!(
            "{what}: {} {}",
            r.status,
            String::from_utf8_lossy(&r.body)
        ))
    }
}

fn query(tags: &BTreeMap<String, String>) -> String {
    tags.iter()
        .map(|(k, v)| format!("{}={}", enc(k, false), enc(v, false)))
        .collect::<Vec<_>>()
        .join("&")
}

/// Replays the buckets and objects in `dir` (Go's `s3/`), skipping buckets named `s3:<bucket>` in `finished`.
pub(super) fn replay(
    dir: &Path,
    client: &Client,
    finished: &[String],
    r: &mut Replayed,
    mark: &dyn Fn(&str) -> Result<(), String>,
) -> Result<(), String> {
    let file = dir.join("meta.bolt");
    if !file.is_file() {
        return Ok(());
    }
    let db = doze_bolt::Db::open(&file).map_err(|e| format!("s3/meta.bolt: {e}"))?;
    let list = client.signed("s3", "GET", "/", &[], b"")?;
    ok("listing buckets", &list)?;
    let there: Vec<String> = xml_all(&String::from_utf8_lossy(&list.body), "Name")
        .into_iter()
        .map(str::to_owned)
        .collect();
    for (_, value) in db.values(&[b"buckets"]).map_err(|e| e.to_string())? {
        let b: GoBucket = serde_json::from_slice(&value).map_err(|e| format!("a bucket: {e}"))?;
        let done = format!("s3:{}", b.name);
        if finished.contains(&done) {
            continue;
        }
        let path = format!("/{}", enc(&b.name, false));
        // A bucket doze.hcl declares is the stack's: its settings come from there. One only Go knew is made as Go had it.
        if !there.contains(&b.name) {
            make(client, &b, &path, r)?;
        }
        r.objects += objects(&db, dir, client, &b.name, &path, r)?;
        if !db
            .items(&[format!("up:{}", b.name).as_bytes()])
            .map_err(|e| e.to_string())?
            .is_empty()
        {
            r.left
                .push(format!("bucket {}: multipart uploads in progress", b.name));
        }
        r.buckets += 1;
        mark(&done)?;
    }
    Ok(())
}

/// Every version of every object in `bucket`, oldest first; how many.
fn objects(
    db: &doze_bolt::Db,
    dir: &Path,
    client: &Client,
    bucket: &str,
    path: &str,
    r: &mut Replayed,
) -> Result<usize, String> {
    let mut versions = db
        .values(&[format!("ver:{bucket}").as_bytes()])
        .map_err(|e| e.to_string())?;
    // Keys are `key \0 ^seq`: a key's newest version first. Reversed, every key's versions come oldest first.
    versions.reverse();
    let mut n = 0;
    for (_, value) in versions {
        let v: GoVersion =
            serde_json::from_slice(&value).map_err(|e| format!("an object in {bucket}: {e}"))?;
        let target = format!("{path}/{}", enc(&v.key, true));
        if v.delete_marker {
            ok(
                "a delete marker",
                &client.signed("s3", "DELETE", &target, &[], b"")?,
            )?;
            continue;
        }
        let body = std::fs::read(dir.join(&v.blob))
            .map_err(|e| format!("object {bucket}/{}: {} ({e})", v.key, v.blob))?;
        let mut headers: Vec<(String, String)> = Vec::new();
        if !v.content_type.is_empty() {
            headers.push(("content-type".into(), v.content_type.clone()));
        }
        for (k, val) in &v.meta {
            headers.push((
                format!("x-amz-meta-{}", k.to_ascii_lowercase()),
                val.clone(),
            ));
        }
        for (k, val) in &v.headers {
            headers.push((k.to_ascii_lowercase(), val.clone()));
        }
        if !v.tags.is_empty() {
            headers.push(("x-amz-tagging".into(), query(&v.tags)));
        }
        if !v.storage_class.is_empty() && v.storage_class != "STANDARD" {
            headers.push(("x-amz-storage-class".into(), v.storage_class.clone()));
        }
        if !v.retain_mode.is_empty() || v.legal_hold {
            r.left.push(format!(
                "object {bucket}/{}: its lock (retention or legal hold)",
                v.key
            ));
        }
        ok(
            &format!("object {bucket}/{}", v.key),
            &client.signed("s3", "PUT", &target, &headers, &body)?,
        )?;
        n += 1;
    }
    Ok(n)
}

/// `s` as XML text.
fn xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Makes a bucket Go had and doze.hcl does not declare, with its versioning and tags.
fn make(client: &Client, b: &GoBucket, path: &str, r: &mut Replayed) -> Result<(), String> {
    ok(
        "creating a bucket",
        &client.signed("s3", "PUT", path, &[], b"")?,
    )?;
    if !b.versioning.is_empty() {
        let body = format!(
            "<VersioningConfiguration xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\"><Status>{}</Status></VersioningConfiguration>",
            xml(&b.versioning)
        );
        let reply = client.signed(
            "s3",
            "PUT",
            &format!("{path}?versioning"),
            &[],
            body.as_bytes(),
        )?;
        ok("bucket versioning", &reply)?;
    }
    if !b.tags.is_empty() {
        let tags: String = b
            .tags
            .iter()
            .map(|(k, v)| format!("<Tag><Key>{}</Key><Value>{}</Value></Tag>", xml(k), xml(v)))
            .collect();
        let body = format!("<Tagging><TagSet>{tags}</TagSet></Tagging>");
        let reply = client.signed(
            "s3",
            "PUT",
            &format!("{path}?tagging"),
            &[],
            body.as_bytes(),
        )?;
        ok("bucket tags", &reply)?;
    }
    let more = [
        &b.cors,
        &b.lifecycle,
        &b.website,
        &b.policy,
        &b.encryption,
        &b.notification,
        &b.replication,
    ];
    if more.iter().any(|s| !s.is_empty()) {
        r.left.push(format!(
            "bucket {}: settings beyond versioning and tags",
            b.name
        ));
    }
    Ok(())
}
