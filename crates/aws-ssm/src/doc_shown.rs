//! How a document is shown: `DescribeDocument`'s description, a row of `ListDocumentVersions`, and the documents it requires.

use aws_model_ssm::keys::{DocumentDescription as Shown, DocumentParameter, DocumentRequires, Tag};
use aws_schema::build::Object;
use aws_schema::view::Key;
use aws_schema::view::kind::{List, Obj};

use crate::document::{DocVersion, Document};
use crate::read::seconds;

pub(crate) fn put_requires<S>(
    o: &mut Object<'_, S>,
    key: Key<S, List<Obj<DocumentRequires>>>,
    ver: &DocVersion,
) {
    if ver.requires.is_empty() {
        return;
    }
    o.put_list(key, |l| {
        for (n, version) in &ver.requires {
            l.push(|r: &mut Object<'_, DocumentRequires>| {
                r.put(DocumentRequires::NAME, n);
                if let Some(x) = version {
                    r.put(DocumentRequires::VERSION, x);
                }
            });
        }
    });
}

pub(crate) fn version_info(
    i: &mut Object<'_, aws_model_ssm::keys::DocumentVersionInfo>,
    name: &str,
    doc: &Document,
    x: &DocVersion,
) {
    use aws_model_ssm::keys::DocumentVersionInfo as I;
    i.put(I::NAME, name);
    if let Some(d) = &x.display_name {
        i.put(I::DISPLAY_NAME, d);
    }
    i.put(I::DOCUMENT_VERSION, &x.number.to_string());
    if let Some(n) = &x.version_name {
        i.put(I::VERSION_NAME, n);
    }
    i.put_time(I::CREATED_DATE, seconds(x.created_ms));
    i.put_bool(I::IS_DEFAULT_VERSION, x.number == doc.default_version);
    i.put(I::DOCUMENT_FORMAT, &x.format);
    i.put(I::STATUS, "Active");
    if let Some(r) = &x.review {
        i.put(I::REVIEW_STATUS, r);
    }
}

/// A document version as `DescribeDocument` shows it.
pub(crate) fn shown(
    d: &mut Object<'_, Shown>,
    name: &str,
    doc: &Document,
    ver: &DocVersion,
    status: &str,
) {
    d.put(Shown::HASH, &ver.hash);
    d.put(Shown::HASH_TYPE, "Sha256");
    d.put(Shown::NAME, name);
    if let Some(x) = &ver.display_name {
        d.put(Shown::DISPLAY_NAME, x);
    }
    if let Some(x) = &ver.version_name {
        d.put(Shown::VERSION_NAME, x);
    }
    d.put(Shown::OWNER, &doc.owner);
    d.put_time(Shown::CREATED_DATE, seconds(ver.created_ms));
    d.put(Shown::STATUS, status);
    d.put(Shown::DOCUMENT_VERSION, &ver.number.to_string());
    if let Some(x) = &ver.description {
        d.put(Shown::DESCRIPTION, x);
    }
    // Absent, not empty, for a document without parameters (recorded: test_ssm.py::test_document_content_is_checked).
    if !ver.params.is_empty() {
        put_params(d, ver);
    }
    d.put_list(Shown::PLATFORM_TYPES, |l| {
        for p in &ver.platforms {
            l.push(p);
        }
    });
    d.put(Shown::DOCUMENT_TYPE, &doc.kind);
    if let Some(x) = &ver.schema_version {
        d.put(Shown::SCHEMA_VERSION, x);
    }
    d.put(
        Shown::LATEST_VERSION,
        &doc.latest().map_or(0, |v| v.number).to_string(),
    );
    d.put(Shown::DEFAULT_VERSION, &doc.default_version.to_string());
    d.put(Shown::DOCUMENT_FORMAT, &ver.format);
    if let Some(x) = &ver.target_type {
        d.put(Shown::TARGET_TYPE, x);
    }
    d.put_list(Shown::TAGS, |l| {
        for (k, v) in &doc.tags {
            l.push(|t: &mut Object<'_, Tag>| {
                t.put(Tag::KEY, k);
                t.put(Tag::VALUE, v);
            });
        }
    });
    put_requires(d, Shown::REQUIRES, ver);
    if let Some(r) = &ver.review {
        d.put(Shown::REVIEW_STATUS, r);
    }
}

fn put_params(d: &mut Object<'_, Shown>, ver: &DocVersion) {
    d.put_list(Shown::PARAMETERS, |l| {
        for p in &ver.params {
            l.push(|o: &mut Object<'_, DocumentParameter>| {
                o.put(DocumentParameter::NAME, &p.name);
                o.put(DocumentParameter::TYPE, &p.kind);
                if let Some(x) = &p.description {
                    o.put(DocumentParameter::DESCRIPTION, x);
                }
                if let Some(x) = &p.default {
                    o.put(DocumentParameter::DEFAULT_VALUE, x);
                }
            });
        }
    });
}
