//! The Access page's API: the page (`GET /api/v1/access`), the Create ACL dialog's call for what was typed
//! (`GET /api/v1/access/acls/new?...`) and its write (`POST /api/v1/access/acls/new`), the Delete ACL dialog with what the broker
//! decides afterwards (`GET /api/v1/access/acls/delete?...`) and its write (`POST /api/v1/access/acls/delete`). The writes are
//! CreateAcls and DeleteAcls as the console's principal through the classic console's own paths; a refusal is Kafka's words.
//! Nothing here can hold a secret: a user is a name, its mechanisms and its iteration counts.

use axum::body::Bytes;
use axum::extract::{RawQuery, State};
use axum::http::StatusCode;
use axum::response::Response;
use serde::{Deserialize, Serialize};

use super::{error, json};
use crate::acl_words::{KINDS, OPERATIONS};
use crate::gather::access_dialogs::{AclFields, entry_of, input_of, missing};
use crate::model::{
    AccessView, AclCreateResult, AclCreateView, AclDeleteResult, AclDeleteView, AclEntry,
};
use crate::state::Shared;

/// A choice of a list of the ACL form: the word the form carries, Kafka's label, its wire number.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AclChoice {
    pub value: String,
    pub label: String,
    pub wire: String,
    pub number: i8,
}

/// `GET /api/v1/access`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AccessResponse {
    pub access: AccessView,
    /// The resource types and the operations, for the form's lists and the table's labels.
    pub kinds: Vec<AclChoice>,
    pub operations: Vec<AclChoice>,
}

fn choices(list: &[(&str, &str, &str, i8)]) -> Vec<AclChoice> {
    list.iter()
        .map(|(v, l, w, n)| AclChoice {
            value: (*v).to_owned(),
            label: (*l).to_owned(),
            wire: (*w).to_owned(),
            number: *n,
        })
        .collect()
}

/// What the ACL dialogs post: the ACL, in the form's words.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct AclPost {
    #[serde(rename = "type")]
    pub kind: String,
    pub name: String,
    pub pattern: String,
    pub principal: String,
    pub host: String,
    pub op: String,
    pub perm: String,
}

impl AclPost {
    fn fields(&self) -> AclFields<'_> {
        AclFields {
            kind: &self.kind,
            name: &self.name,
            pattern: &self.pattern,
            principal: &self.principal,
            host: &self.host,
            op: &self.op,
            perm: &self.perm,
        }
    }

    fn of_query(q: Option<&str>) -> Self {
        let f = crate::form::parse(q.unwrap_or(""));
        let t = |k: &str| crate::form::text(&f, k).to_owned();
        Self {
            kind: t("type"),
            name: t("name"),
            pattern: t("pattern"),
            principal: t("principal"),
            host: t("host"),
            op: t("op"),
            perm: t("perm"),
        }
    }
}

/// What a Create ACL submit came to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum AclCreateOutcome {
    Created,
    /// A field is missing: the dialog with the sentence.
    Invalid { form: AclCreateView },
    /// Kafka said no: the dialog with its words.
    Refused { form: AclCreateView },
}

/// What a Delete ACL submit came to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum AclDeleteOutcome {
    Deleted,
    /// The ACL is no longer there.
    Gone { form: AclDeleteView },
    Refused { form: AclDeleteView },
}

pub(super) async fn page(State(s): State<Shared>, RawQuery(q): RawQuery) -> Response {
    // `?state=secured` is read by the fixtures only; a live broker decides by its data.
    let f = crate::form::parse(q.as_deref().unwrap_or(""));
    json(
        StatusCode::OK,
        &AccessResponse {
            access: s.source.access(crate::form::get(&f, "state")).await,
            kinds: choices(&KINDS),
            operations: choices(&OPERATIONS),
        },
    )
}

pub(super) async fn create_form(State(s): State<Shared>, RawQuery(q): RawQuery) -> Response {
    let p = AclPost::of_query(q.as_deref());
    json(StatusCode::OK, &s.source.acl_create_form(&p.fields()).await)
}

fn posted(body: &[u8]) -> Result<AclPost, Response> {
    serde_json::from_slice::<AclPost>(body).map_err(|_| {
        error(
            StatusCode::BAD_REQUEST,
            "BAD_REQUEST",
            "The ACL was not in the form the console sends.",
        )
    })
}

pub(super) async fn create(State(s): State<Shared>, body: Bytes) -> Response {
    let p = match posted(&body) {
        Ok(p) => p,
        Err(r) => return r,
    };
    let outcome = match s.source.acl_create(&p.fields()).await {
        AclCreateResult::Created => AclCreateOutcome::Created,
        AclCreateResult::Invalid(v) => AclCreateOutcome::Invalid { form: *v },
        AclCreateResult::Refused(v) => AclCreateOutcome::Refused { form: *v },
    };
    json(StatusCode::OK, &outcome)
}

/// The ACL the fields name, or the refusal that says it names none.
fn target(p: &AclPost) -> Result<AclEntry, Response> {
    let e = entry_of(&input_of(&p.fields()));
    match missing(&e) {
        None => Ok(e),
        Some(_) => Err(error(
            StatusCode::BAD_REQUEST,
            "BAD_REQUEST",
            "That does not name an ACL: it needs the resource, the principal and the operation.",
        )),
    }
}

pub(super) async fn delete_form(State(s): State<Shared>, RawQuery(q): RawQuery) -> Response {
    match target(&AclPost::of_query(q.as_deref())) {
        Ok(e) => json(StatusCode::OK, &s.source.acl_delete_form(e).await),
        Err(r) => r,
    }
}

pub(super) async fn delete(State(s): State<Shared>, body: Bytes) -> Response {
    let e = match posted(&body).and_then(|p| target(&p)) {
        Ok(e) => e,
        Err(r) => return r,
    };
    let outcome = match s.source.acl_delete(e).await {
        AclDeleteResult::Deleted => AclDeleteOutcome::Deleted,
        AclDeleteResult::Gone(v) => AclDeleteOutcome::Gone { form: *v },
        AclDeleteResult::Refused(v) => AclDeleteOutcome::Refused { form: *v },
    };
    json(StatusCode::OK, &outcome)
}
