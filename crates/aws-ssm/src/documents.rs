//! SSM documents: `CreateDocument`, `GetDocument`, `DescribeDocument`, `UpdateDocument`, `DeleteDocument`, `ListDocumentVersions`
//! and `UpdateDocumentDefaultVersion`. A document is kept with every version; nothing runs it. A new version is not the default
//! until `UpdateDocumentDefaultVersion` says so, as on AWS. Words not recorded are marked (memory).

use aws_model_ssm::keys::{
    CreateDocumentRequest as Create, CreateDocumentResult as Created,
    DeleteDocumentRequest as Delete, DescribeDocumentRequest as Describe,
    DescribeDocumentResult as Described, DocumentDefaultVersionDescription as DefaultShown,
    DocumentRequires, GetDocumentRequest as Get, GetDocumentResult as Got,
    ListDocumentVersionsRequest as Versions, ListDocumentVersionsResult as VersionsResult, Tag,
    UpdateDocumentDefaultVersionRequest as SetDefault,
    UpdateDocumentDefaultVersionResult as DefaultSet, UpdateDocumentRequest as Update,
    UpdateDocumentResult as Updated,
};
use aws_schema::build::Object;
use aws_schema::view::View;
use aws_wire::ApiError;
use aws_wire::service::{Call, Reply};

use crate::core::{Core, validation};
use crate::doc_shown::{put_requires, shown, version_info};
use crate::document::{self, DocVersion, Document, invalid_document, invalid_version};
use crate::page;
use crate::read::seconds;
use aws_wire::handler::{empty, reply, request};

const VERSIONS_PAGE: usize = 50;

/// The name a request gives: a name, or the document's ARN.
pub(crate) fn bare(name: &str) -> &str {
    name.split_once(":document/").map_or(name, |(_, n)| n)
}

/// What a new version is made of.
struct New<'a> {
    content: &'a str,
    format: &'a str,
    display_name: Option<String>,
    version_name: Option<String>,
    target_type: Option<String>,
    requires: Vec<(String, Option<String>)>,
}

impl Core {
    fn version_of(&self, kind: &str, number: i64, new: New<'_>) -> Result<DocVersion, ApiError> {
        let read = document::read(new.content, new.format, kind)?;
        Ok(DocVersion {
            number,
            content: new.content.to_owned(),
            format: new.format.to_owned(),
            display_name: new.display_name,
            version_name: new.version_name,
            target_type: new.target_type,
            hash: read.hash,
            created_ms: self.now_ms(),
            schema_version: read.schema_version,
            description: read.description,
            params: read.params,
            platforms: read.platforms,
            requires: new.requires,
            review: None,
        })
    }

    pub(crate) fn create_document(&mut self, call: &Call) -> Reply {
        let v = request::<Create>(call)?;
        let name = v.get(Create::NAME).unwrap_or_default();
        document::check_name(name)?;
        if v.get(Create::ATTACHMENTS)
            .is_some_and(|mut a| a.next().is_some())
        {
            return Err(validation(
                "doze-aws does not keep document attachments: create the document without Attachments.",
            ));
        }
        if self.documents.get(name).is_some() {
            // (memory)
            return Err(ApiError::modeled(
                "DocumentAlreadyExists",
                "Document with same name already exists.",
            ));
        }
        let kind = v.get(Create::DOCUMENT_TYPE).unwrap_or("Command").to_owned();
        let requires = v
            .get(Create::REQUIRES)
            .map(|r| {
                r.map(|q| {
                    let n = q.get(DocumentRequires::NAME).unwrap_or_default().to_owned();
                    (n, q.get(DocumentRequires::VERSION).map(str::to_owned))
                })
                .collect()
            })
            .unwrap_or_default();
        let new = New {
            content: v.get(Create::CONTENT).unwrap_or_default(),
            format: v.get(Create::DOCUMENT_FORMAT).unwrap_or("JSON"),
            display_name: v.get(Create::DISPLAY_NAME).map(str::to_owned),
            version_name: v.get(Create::VERSION_NAME).map(str::to_owned),
            target_type: v.get(Create::TARGET_TYPE).map(str::to_owned),
            requires,
        };
        let first = self.version_of(&kind, 1, new)?;
        let tags = v
            .get(Create::TAGS)
            .map(|t| t.map(|t| tag_of(&t)).collect())
            .unwrap_or_default();
        let doc = Document {
            kind,
            owner: call.caller.account_id.clone(),
            created_ms: first.created_ms,
            versions: vec![first],
            default_version: 1,
            tags,
            shared: Vec::new(),
            shared_version: None,
        };
        self.documents.insert(name.to_owned(), doc.clone());
        let latest = doc.latest().ok_or_else(invalid_document)?;
        reply::<Created>(|o| {
            o.put_object(Created::DOCUMENT_DESCRIPTION, |d| {
                shown(d, name, &doc, latest, "Creating")
            });
        })
    }

    pub(crate) fn get_document(&mut self, call: &Call) -> Reply {
        let v = request::<Get>(call)?;
        let name = bare(v.get(Get::NAME).unwrap_or_default());
        let doc = self.documents.get(name).ok_or_else(invalid_document)?;
        let ver = doc.pick(v.get(Get::DOCUMENT_VERSION), v.get(Get::VERSION_NAME))?;
        let format = v.get(Get::DOCUMENT_FORMAT);
        let content = document::content_as(ver, format);
        reply::<Got>(|o| {
            o.put(Got::NAME, name);
            o.put_time(Got::CREATED_DATE, seconds(ver.created_ms));
            if let Some(d) = &ver.display_name {
                o.put(Got::DISPLAY_NAME, d);
            }
            if let Some(n) = &ver.version_name {
                o.put(Got::VERSION_NAME, n);
            }
            o.put(Got::DOCUMENT_VERSION, &ver.number.to_string());
            o.put(Got::STATUS, "Active");
            o.put(Got::CONTENT, &content);
            o.put(Got::DOCUMENT_TYPE, &doc.kind);
            o.put(Got::DOCUMENT_FORMAT, format.unwrap_or(&ver.format));
            put_requires(o, Got::REQUIRES, ver);
            if let Some(r) = &ver.review {
                o.put(Got::REVIEW_STATUS, r);
            }
        })
    }

    pub(crate) fn describe_document(&mut self, call: &Call) -> Reply {
        let v = request::<Describe>(call)?;
        let name = bare(v.get(Describe::NAME).unwrap_or_default());
        let doc = self.documents.get(name).ok_or_else(invalid_document)?;
        let ver = doc.pick(
            v.get(Describe::DOCUMENT_VERSION),
            v.get(Describe::VERSION_NAME),
        )?;
        reply::<Described>(|o| {
            o.put_object(Described::DOCUMENT, |d| shown(d, name, doc, ver, "Active"));
        })
    }

    pub(crate) fn update_document(&mut self, call: &Call) -> Reply {
        let v = request::<Update>(call)?;
        let name = v.get(Update::NAME).unwrap_or_default();
        if v.get(Update::ATTACHMENTS)
            .is_some_and(|mut a| a.next().is_some())
        {
            return Err(validation(
                "doze-aws does not keep document attachments: update the document without Attachments.",
            ));
        }
        let doc = self.documents.get(name).ok_or_else(invalid_document)?;
        let latest = doc.latest().ok_or_else(invalid_document)?;
        match v.get(Update::DOCUMENT_VERSION) {
            None | Some("$LATEST") => {}
            Some(n) if n == latest.number.to_string() => {}
            // (memory) only the latest version can be updated.
            Some(_) => return Err(invalid_version()),
        }
        if doc.versions.len() >= document::MAX_VERSIONS {
            // (memory)
            return Err(ApiError::modeled(
                "DocumentVersionLimitExceeded",
                "The document has too many versions. Delete one and try again.",
            ));
        }
        let version_name = v.get(Update::VERSION_NAME).map(str::to_owned);
        if version_name.is_some() && doc.versions.iter().any(|x| x.version_name == version_name) {
            // (memory)
            return Err(ApiError::modeled(
                "DuplicateDocumentVersionName",
                "The version name has already been used in this document. Specify a different version name, and then try again.",
            ));
        }
        let new = New {
            content: v.get(Update::CONTENT).unwrap_or_default(),
            format: v.get(Update::DOCUMENT_FORMAT).unwrap_or("JSON"),
            display_name: v
                .get(Update::DISPLAY_NAME)
                .map(str::to_owned)
                .or_else(|| latest.display_name.clone()),
            version_name,
            target_type: v
                .get(Update::TARGET_TYPE)
                .map(str::to_owned)
                .or_else(|| latest.target_type.clone()),
            requires: latest.requires.clone(),
        };
        let next = self.version_of(&doc.kind, latest.number + 1, new)?;
        if next.hash == latest.hash {
            // (memory)
            return Err(ApiError::modeled(
                "DuplicateDocumentContent",
                "The content of the association document matches another document. Change the content of the document and try again.",
            ));
        }
        let doc = self.documents.get_mut(name).ok_or_else(invalid_document)?;
        doc.versions.push(next);
        let doc = doc.clone();
        let ver = doc.latest().ok_or_else(invalid_document)?;
        reply::<Updated>(|o| {
            o.put_object(Updated::DOCUMENT_DESCRIPTION, |d| {
                shown(d, name, &doc, ver, "Updating")
            });
        })
    }

    pub(crate) fn delete_document(&mut self, call: &Call) -> Reply {
        let v = request::<Delete>(call)?;
        let name = v.get(Delete::NAME).unwrap_or_default();
        let (version, version_name) =
            (v.get(Delete::DOCUMENT_VERSION), v.get(Delete::VERSION_NAME));
        let doc = self.documents.get(name).ok_or_else(invalid_document)?;
        if version.is_none() && version_name.is_none() {
            self.documents.remove(name);
            return empty();
        }
        let number = doc.pick(version, version_name)?.number;
        if number == doc.default_version {
            // (memory)
            return Err(ApiError::modeled(
                "InvalidDocumentOperation",
                "You can't delete the default version of a document. Make another version the default, or delete the whole document.",
            ));
        }
        if let Some(doc) = self.documents.get_mut(name) {
            doc.versions.retain(|x| x.number != number);
        }
        empty()
    }

    pub(crate) fn list_document_versions(&mut self, call: &Call) -> Reply {
        let v = request::<Versions>(call)?;
        let name = bare(v.get(Versions::NAME).unwrap_or_default());
        let limit = v.get(Versions::MAX_RESULTS).map_or(VERSIONS_PAGE, |n| {
            usize::try_from(n).unwrap_or(VERSIONS_PAGE)
        });
        let after: i64 = match v.get(Versions::NEXT_TOKEN) {
            Some(t) => page::resume(t)?.parse().map_err(|_| {
                ApiError::modeled("InvalidNextToken", "The specified token is not valid.")
            })?,
            None => 0,
        };
        let doc = self.documents.get(name).ok_or_else(invalid_document)?;
        let mut rows: Vec<&DocVersion> = doc
            .versions
            .iter()
            .filter(|x| x.number > after)
            .take(limit + 1)
            .collect();
        let more = rows.len() > limit;
        rows.truncate(limit);
        let token = more
            .then(|| rows.last().map(|x| page::token(&x.number.to_string())))
            .flatten();
        reply::<VersionsResult>(|o| {
            o.put_list(VersionsResult::DOCUMENT_VERSIONS, |l| {
                for x in &rows {
                    l.push(|i| version_info(i, name, doc, x));
                }
            });
            if let Some(t) = &token {
                o.put(VersionsResult::NEXT_TOKEN, t);
            }
        })
    }

    pub(crate) fn update_document_default_version(&mut self, call: &Call) -> Reply {
        let v = request::<SetDefault>(call)?;
        let name = v.get(SetDefault::NAME).unwrap_or_default();
        let wanted = v.get(SetDefault::DOCUMENT_VERSION).unwrap_or_default();
        let doc = self.documents.get_mut(name).ok_or_else(invalid_document)?;
        let number = wanted
            .parse()
            .ok()
            .filter(|n| doc.version(*n).is_some())
            .ok_or_else(invalid_version)?;
        doc.default_version = number;
        let version_name = doc.version(number).and_then(|x| x.version_name.clone());
        reply::<DefaultSet>(|o| {
            o.put_object(
                DefaultSet::DESCRIPTION,
                |d: &mut Object<'_, DefaultShown>| {
                    d.put(DefaultShown::NAME, name);
                    d.put(DefaultShown::DEFAULT_VERSION, wanted);
                    if let Some(n) = &version_name {
                        d.put(DefaultShown::DEFAULT_VERSION_NAME, n);
                    }
                },
            );
        })
    }
}

pub(crate) fn tag_of(t: &View<'_, Tag>) -> (String, String) {
    (
        t.get(Tag::KEY).unwrap_or_default().to_owned(),
        t.get(Tag::VALUE).unwrap_or_default().to_owned(),
    )
}
