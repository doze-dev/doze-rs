//! Finding and sharing documents: `ListDocuments`, `DescribeDocumentPermission`, `ModifyDocumentPermission` (the review of a change
//! template is `doc_review.rs`). Only the account's own documents exist here: the hundreds AWS owns (`AWS-RunShellScript`)
//! are not served, so a list filtered to `Owner=Amazon` is empty. Words not recorded are marked (memory).

use aws_model_ssm::keys::{
    DescribeDocumentPermissionRequest as DescribePerm, DescribeDocumentPermissionResponse as Perm,
    DocumentFilter, DocumentIdentifier as Ident, DocumentKeyValuesFilter as KvFilter,
    ListDocumentsRequest as List, ListDocumentsResult as Listed,
    ModifyDocumentPermissionRequest as Modify, Tag,
};
use aws_schema::build::Object;
use aws_wire::ApiError;
use aws_wire::service::{Call, Reply};

use crate::core::Core;
use crate::document::{Document, invalid_document};
use crate::page;
use crate::read::seconds;
use aws_wire::handler::{empty, reply, request};

const LIST_PAGE: usize = 50;
/// The most accounts a document is shared with (the model: up to 20 in a call; AWS's quota, memory).
const MAX_SHARED: usize = 20;

/// Whether `doc` called `name` passes the filter `key` = any of `values`.
fn holds(key: &str, values: &[String], name: &str, doc: &Document) -> Result<bool, ApiError> {
    let latest = doc.latest();
    let any = |f: &dyn Fn(&str) -> bool| values.iter().any(|v| f(v));
    Ok(match key {
        "Owner" => any(&|v| match v {
            "Self" | "All" => true,
            "Public" => doc.shared.iter().any(|a| a == "all"),
            _ => v == doc.owner,
        }),
        "Name" => any(&|v| name.starts_with(v)),
        "DocumentType" => any(&|v| v == doc.kind),
        "PlatformTypes" => any(&|v| latest.is_some_and(|l| l.platforms.iter().any(|p| p == v))),
        "SearchKeyword" => any(&|v| name.to_ascii_lowercase().contains(&v.to_ascii_lowercase())),
        "TargetType" => any(&|v| latest.and_then(|l| l.target_type.as_deref()) == Some(v)),
        "SchemaVersion" => any(&|v| latest.and_then(|l| l.schema_version.as_deref()) == Some(v)),
        other => match other.strip_prefix("tag:") {
            Some(tag) => doc
                .tags
                .get(tag)
                .is_some_and(|t| values.is_empty() || values.contains(t)),
            // (memory)
            None => {
                return Err(ApiError::modeled(
                    "InvalidFilterKey",
                    format!("The filter key {other} is not valid."),
                ));
            }
        },
    })
}

impl Core {
    pub(crate) fn list_documents(&mut self, call: &Call) -> Reply {
        let v = request::<List>(call)?;
        let limit = v
            .get(List::MAX_RESULTS)
            .map_or(LIST_PAGE, |n| usize::try_from(n).unwrap_or(LIST_PAGE));
        let after = v.get(List::NEXT_TOKEN).map(page::resume).transpose()?;
        let mut filters: Vec<(String, Vec<String>)> = v
            .get(List::FILTERS)
            .into_iter()
            .flatten()
            .map(|f| {
                let values = f
                    .get(KvFilter::VALUES)
                    .map(|x| x.map(str::to_owned).collect())
                    .unwrap_or_default();
                (f.get(KvFilter::KEY).unwrap_or_default().to_owned(), values)
            })
            .collect();
        for f in v.get(List::DOCUMENT_FILTER_LIST).into_iter().flatten() {
            let key = f.get(DocumentFilter::KEY).unwrap_or_default().to_owned();
            filters.push((
                key,
                vec![f.get(DocumentFilter::VALUE).unwrap_or_default().to_owned()],
            ));
        }
        let mut rows = Vec::new();
        for (name, doc) in self
            .documents
            .iter()
            .filter(|(n, _)| after.as_deref().is_none_or(|a| *n > a))
        {
            let mut keep = true;
            for (key, values) in &filters {
                keep &= holds(key, values, name, doc)?;
            }
            if keep {
                rows.push((name, doc));
                if rows.len() > limit {
                    break;
                }
            }
        }
        let more = rows.len() > limit;
        rows.truncate(limit);
        let token = more
            .then(|| rows.last().map(|(n, _)| page::token(n)))
            .flatten();
        reply::<Listed>(|o| {
            o.put_list(Listed::DOCUMENT_IDENTIFIERS, |l| {
                for (name, doc) in &rows {
                    l.push(|i| identifier(i, name, doc));
                }
            });
            if let Some(t) = &token {
                o.put(Listed::NEXT_TOKEN, t);
            }
        })
    }

    pub(crate) fn describe_document_permission(&mut self, call: &Call) -> Reply {
        let v = request::<DescribePerm>(call)?;
        let doc = self
            .documents
            .get(v.get(DescribePerm::NAME).unwrap_or_default())
            .ok_or_else(|| invalid_document(v.get(DescribePerm::NAME).unwrap_or_default()))?;
        let version = doc
            .shared_version
            .clone()
            .unwrap_or_else(|| "$DEFAULT".to_owned());
        reply::<Perm>(|o| {
            o.put_list(Perm::ACCOUNT_IDS, |l| {
                for a in &doc.shared {
                    l.push(a);
                }
            });
            o.put_list(Perm::ACCOUNT_SHARING_INFO_LIST, |l| {
                for a in &doc.shared {
                    l.push(
                        |i: &mut Object<'_, aws_model_ssm::keys::AccountSharingInfo>| {
                            i.put(aws_model_ssm::keys::AccountSharingInfo::ACCOUNT_ID, a);
                            i.put(
                                aws_model_ssm::keys::AccountSharingInfo::SHARED_DOCUMENT_VERSION,
                                &version,
                            );
                        },
                    );
                }
            });
        })
    }

    pub(crate) fn modify_document_permission(&mut self, call: &Call) -> Reply {
        let v = request::<Modify>(call)?;
        let norm = |a: &str| {
            if a.eq_ignore_ascii_case("all") {
                "all".to_owned()
            } else {
                a.to_owned()
            }
        };
        let add: Vec<String> = v
            .get(Modify::ACCOUNT_IDS_TO_ADD)
            .map(|x| x.map(norm).collect())
            .unwrap_or_default();
        let remove: Vec<String> = v
            .get(Modify::ACCOUNT_IDS_TO_REMOVE)
            .map(|x| x.map(norm).collect())
            .unwrap_or_default();
        if add.iter().any(|a| a == "all")
            && self
                .setting("/ssm/documents/console/public-sharing-permission")
                .as_deref()
                == Some("Disable")
        {
            // (memory) the account's setting blocks sharing a document with everyone.
            return Err(ApiError::modeled(
                "InvalidPermissionType",
                "Public sharing of documents is disabled for this account by the service setting /ssm/documents/console/public-sharing-permission.",
            ));
        }
        let shared_version = v.get(Modify::SHARED_DOCUMENT_VERSION).map(str::to_owned);
        let doc = self
            .documents
            .get_mut(v.get(Modify::NAME).unwrap_or_default())
            .ok_or_else(|| invalid_document(v.get(Modify::NAME).unwrap_or_default()))?;
        let mut next = doc.shared.clone();
        next.retain(|a| !remove.contains(a));
        for a in add {
            if !next.contains(&a) {
                next.push(a);
            }
        }
        if next.len() > MAX_SHARED {
            // (memory)
            return Err(ApiError::modeled(
                "DocumentPermissionLimit",
                "The document cannot be shared with more AWS accounts. You can share a document with a maximum of 20 accounts.",
            ));
        }
        doc.shared = next;
        if shared_version.is_some() {
            doc.shared_version = shared_version;
        }
        empty()
    }
}

fn identifier(i: &mut Object<'_, Ident>, name: &str, doc: &Document) {
    i.put(Ident::NAME, name);
    i.put_time(Ident::CREATED_DATE, seconds(doc.created_ms));
    i.put(Ident::OWNER, &doc.owner);
    i.put(Ident::DOCUMENT_TYPE, &doc.kind);
    i.put(Ident::DOCUMENT_VERSION, &doc.default_version.to_string());
    if let Some(v) = doc.version(doc.default_version) {
        if let Some(x) = &v.display_name {
            i.put(Ident::DISPLAY_NAME, x);
        }
        // No VersionName, even when the default version has one (recorded: test_ssm.py::test_document_lifecycle).
        i.put_list(Ident::PLATFORM_TYPES, |l| {
            for p in &v.platforms {
                l.push(p);
            }
        });
        if let Some(x) = &v.schema_version {
            i.put(Ident::SCHEMA_VERSION, x);
        }
        i.put(Ident::DOCUMENT_FORMAT, &v.format);
        if let Some(x) = &v.target_type {
            i.put(Ident::TARGET_TYPE, x);
        }
        if let Some(r) = &v.review {
            i.put(Ident::REVIEW_STATUS, r);
        }
    }
    i.put_list(Ident::TAGS, |l| {
        for (k, v) in &doc.tags {
            l.push(|t: &mut Object<'_, Tag>| {
                t.put(Tag::KEY, k);
                t.put(Tag::VALUE, v);
            });
        }
    });
}
