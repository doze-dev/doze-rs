//! The review of a change template (Change Manager): `UpdateDocumentMetadata` and `ListDocumentMetadataHistory`. Each action on a
//! version is kept as a reviewer response (when, the status it led to, the comment, who), and the version's review status is the
//! last one's. As the model says (`ReviewStatus`): only one version is `APPROVED` at a time, the one approved before it becoming
//! `REJECTED`; only one is `PENDING` at a time. Who acted is the account (its root's user ID is the account id): doze-aws does not
//! ask IAM for a user name here. Words not recorded are (memory).

use aws_model_ssm::keys::{
    DocumentMetadataResponseInfo as Info, DocumentReviewCommentSource as CommentIn,
    DocumentReviewCommentSource as CommentOut, DocumentReviewerResponseSource as Response,
    DocumentReviews, ListDocumentMetadataHistoryRequest as History,
    ListDocumentMetadataHistoryResponse as Histories, UpdateDocumentMetadataRequest as Meta,
};
use aws_wire::ApiError;
use aws_wire::handler::{empty, reply, request};
use aws_wire::service::{Call, Reply};
use serde::{Deserialize, Serialize};

use crate::core::Core;
use crate::document::invalid_document;
use crate::page;
use crate::read::seconds;

/// The page `ListDocumentMetadataHistory` gives when the request names none (the model's most).
const PAGE: usize = 50;
/// The most responses one version keeps (memory: AWS states no quota; a bound keeps a record small).
const MAX_RESPONSES: usize = 100;

/// One action on a version's review.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ReviewResponse {
    pub(crate) created_ms: i64,
    pub(crate) updated_ms: i64,
    pub(crate) status: String,
    pub(crate) comment: Option<String>,
    pub(crate) reviewer: String,
}

/// The review of one version: who sent it for review, and every response, oldest first.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct Reviews {
    pub(crate) author: Option<String>,
    pub(crate) responses: Vec<ReviewResponse>,
}

/// The key a version's review is kept under.
pub(crate) fn review_key(document: &str, number: i64) -> String {
    format!("{document}/{number}")
}

fn refused(message: impl Into<String>) -> ApiError {
    ApiError::modeled("InvalidDocumentOperation", message)
}

impl Core {
    pub(crate) fn update_document_metadata(&mut self, call: &Call) -> Reply {
        let v = request::<Meta>(call)?;
        let name = v.get(Meta::NAME).unwrap_or_default();
        let reviews = v.get(Meta::DOCUMENT_REVIEWS);
        let action = reviews
            .and_then(|r| r.get(DocumentReviews::ACTION))
            .unwrap_or_default();
        let comment = reviews
            .and_then(|r| r.get(DocumentReviews::COMMENT))
            .into_iter()
            .flatten()
            .find_map(|c| c.get(CommentIn::CONTENT).map(str::to_owned));
        let doc = self
            .documents
            .get(name)
            .ok_or_else(|| invalid_document(name))?;
        if doc.kind != "Automation.ChangeTemplate" {
            return Err(refused(
                "Only a change template (document type Automation.ChangeTemplate) has review metadata.",
            ));
        }
        let number = doc.pick(v.get(Meta::DOCUMENT_VERSION), None)?.number;
        let others: Vec<(i64, Option<String>)> = doc
            .versions
            .iter()
            .filter(|x| x.number != number)
            .map(|x| (x.number, x.review.clone()))
            .collect();
        let now = self.now_ms();
        let who = call.caller.account_id.clone();
        let status = match action {
            "Approve" => "APPROVED",
            "Reject" => "REJECTED",
            "UpdateReview" => return self.update_review(name, number, comment, now),
            _ => {
                if let Some((other, _)) =
                    others.iter().find(|(_, r)| r.as_deref() == Some("PENDING"))
                {
                    return Err(refused(format!(
                        "Version {other} of {name} is already in review. Only one version of a document can be in review at a time."
                    )));
                }
                "PENDING"
            }
        };
        if status == "APPROVED" {
            for (other, _) in others
                .iter()
                .filter(|(_, r)| r.as_deref() == Some("APPROVED"))
            {
                self.set_review(name, *other, "REJECTED");
            }
        }
        let k = review_key(name, number);
        let mut held = self.reviews.get(&k).cloned().unwrap_or_default();
        if status == "PENDING" {
            held.author = Some(who.clone());
        }
        if held.responses.len() >= MAX_RESPONSES {
            held.responses.remove(0);
        }
        held.responses.push(ReviewResponse {
            created_ms: now,
            updated_ms: now,
            status: status.to_owned(),
            comment,
            reviewer: who,
        });
        self.reviews.insert(k, held);
        self.set_review(name, number, status);
        empty()
    }

    /// `UpdateReview`: the comment of the version's last response changes; its status does not.
    fn update_review(
        &mut self,
        name: &str,
        number: i64,
        comment: Option<String>,
        now: i64,
    ) -> Reply {
        let k = review_key(name, number);
        let Some(mut held) = self.reviews.get(&k).cloned() else {
            return Err(refused(format!(
                "Version {number} of {name} has no review to update. Send it for review first."
            )));
        };
        if let Some(last) = held.responses.last_mut() {
            last.updated_ms = now;
            if comment.is_some() {
                last.comment = comment;
            }
        }
        self.reviews.insert(k, held);
        empty()
    }

    fn set_review(&mut self, name: &str, number: i64, status: &str) {
        if let Some(ver) = self
            .documents
            .get_mut(name)
            .and_then(|d| d.versions.iter_mut().find(|x| x.number == number))
        {
            ver.review = Some(status.to_owned());
        }
    }

    pub(crate) fn list_document_metadata_history(&mut self, call: &Call) -> Reply {
        let v = request::<History>(call)?;
        let name = v.get(History::NAME).unwrap_or_default();
        let doc = self
            .documents
            .get(name)
            .ok_or_else(|| invalid_document(name))?;
        let number = doc.pick(v.get(History::DOCUMENT_VERSION), None)?.number;
        let limit = v
            .get(History::MAX_RESULTS)
            .and_then(|n| usize::try_from(n).ok())
            .unwrap_or(PAGE);
        let skip = match v.get(History::NEXT_TOKEN) {
            Some(t) => page::resume(t)?.parse::<usize>().map_err(|_| {
                ApiError::modeled("InvalidNextToken", "The specified token is not valid.")
            })?,
            None => 0,
        };
        let held = self.reviews.get(&review_key(name, number));
        let all = held.map_or(&[][..], |h| h.responses.as_slice());
        let rest = all.get(skip..).unwrap_or_default();
        let shown = rest.get(..limit.min(rest.len())).unwrap_or_default();
        let token =
            (rest.len() > shown.len()).then(|| page::token(&(skip + shown.len()).to_string()));
        let author = held.and_then(|h| h.author.clone());
        reply::<Histories>(|o| {
            o.put(Histories::NAME, name);
            o.put(Histories::DOCUMENT_VERSION, &number.to_string());
            if let Some(a) = &author {
                o.put(Histories::AUTHOR, a);
            }
            o.put_object(Histories::METADATA, |m| {
                m.put_list(Info::REVIEWER_RESPONSE, |l| {
                    for r in shown {
                        l.push(|x| response(x, r));
                    }
                });
            });
            if let Some(t) = &token {
                o.put(Histories::NEXT_TOKEN, t);
            }
        })
    }
}

fn response(x: &mut aws_schema::build::Object<'_, Response>, r: &ReviewResponse) {
    x.put_time(Response::CREATE_TIME, seconds(r.created_ms));
    x.put_time(Response::UPDATED_TIME, seconds(r.updated_ms));
    x.put(Response::REVIEW_STATUS, &r.status);
    x.put(Response::REVIEWER, &r.reviewer);
    x.put_list(Response::COMMENT, |l| {
        if let Some(c) = &r.comment {
            l.push(|e| {
                e.put(CommentOut::TYPE, "Comment");
                e.put(CommentOut::CONTENT, c);
            });
        }
    });
}
