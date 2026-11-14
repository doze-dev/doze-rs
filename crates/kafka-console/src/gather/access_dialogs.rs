//! The two ACL dialogs: Create ACL (CreateAcls) and Delete ACL (DeleteAcls with every field of the one ACL named, so no other
//! can match). A refusal is the broker's own code and sentence. Nothing typed is trusted: a word that is not one of the lists
//! reads as the first of its list, and a text is cut to a length no ACL needs.

use kafka_auth::authz::Rules;
use kafka_wire::api::Response;
use kafka_wire::messages::ApiKey;

use super::access::{CONSOLE_PRINCIPAL, entries};
use crate::acl_words::{
    after_delete, kind_form, kind_from_form, kind_label, kind_wire, operation_form,
    operation_from_form, operation_wire,
};
use crate::backend::{Backend, requests::acls};
use crate::calls::visible;
use crate::model::{
    AclCreateResult, AclCreateView, AclDeleteResult, AclDeleteView, AclEntry, AclInput, AclPattern,
    AclPermission, Lines, Tok,
};
use crate::refusal::refusal;

/// The longest text a field takes.
pub const MOST_TEXT: usize = 512;

/// What a form carried: every field as text.
#[derive(Debug, Clone, Copy, Default)]
pub struct AclFields<'a> {
    pub kind: &'a str,
    pub name: &'a str,
    pub pattern: &'a str,
    pub principal: &'a str,
    pub host: &'a str,
    pub op: &'a str,
    pub perm: &'a str,
}

fn cut(s: &str) -> String {
    s.trim().chars().take(MOST_TEXT).collect()
}

/// The form as the dialog holds it: a word that is none of its list is the first of the list, the host is `*` when empty.
#[must_use]
pub fn input_of(f: &AclFields<'_>) -> AclInput {
    let kind = kind_from_form(f.kind).map_or("topic", kind_form);
    let host = cut(f.host);
    AclInput {
        kind: kind.to_owned(),
        name: cut(f.name),
        pattern: AclPattern::from_form(f.pattern).form().to_owned(),
        principal: cut(f.principal),
        host: if host.is_empty() {
            "*".to_owned()
        } else {
            host
        },
        operation: operation_from_form(f.op)
            .map_or("read", operation_form)
            .to_owned(),
        permission: AclPermission::from_form(f.perm).form().to_owned(),
    }
}

/// The ACL a dialog's input names. The CLUSTER resource has one name, which an empty field means.
#[must_use]
pub fn entry_of(i: &AclInput) -> AclEntry {
    let kind = kind_from_form(&i.kind).unwrap_or(kafka_auth::authz::resource::TOPIC);
    let name = if i.name.is_empty() && kind == kafka_auth::authz::resource::CLUSTER {
        kafka_auth::authz::CLUSTER_NAME.to_owned()
    } else {
        i.name.clone()
    };
    AclEntry {
        kind,
        name,
        pattern: AclPattern::from_form(&i.pattern),
        principal: i.principal.clone(),
        host: i.host.clone(),
        operation: operation_from_form(&i.operation).unwrap_or(kafka_auth::authz::operation::READ),
        permission: AclPermission::from_form(&i.permission),
    }
}

fn row(label: &str, value: &str) -> Vec<(Tok, String)> {
    vec![
        (Tok::Plain, format!("    {label}: ")),
        (Tok::Str, visible(value, 80)),
    ]
}

/// The call, as the boards draw it: the API and its version, then each field.
#[must_use]
pub fn create_call(version: i16, e: &AclEntry) -> Lines {
    vec![
        vec![
            (Tok::Keyword, "CreateAcls".to_owned()),
            (Tok::Plain, format!(" v{version}")),
        ],
        vec![(Tok::Plain, "  creations[0]".to_owned())],
        row("resource_type", &kind_wire(e.kind)),
        row("resource_name", &e.name),
        row("resource_pattern_type", e.pattern.wire()),
        row("principal", &e.principal),
        row("host", &e.host),
        row("operation", &operation_wire(e.operation)),
        row("permission_type", e.permission.wire()),
    ]
}

/// The delete call: one filter that names every field.
#[must_use]
pub fn delete_call(version: i16, e: &AclEntry) -> Lines {
    vec![
        vec![
            (Tok::Keyword, "DeleteAcls".to_owned()),
            (Tok::Plain, format!(" v{version}")),
        ],
        vec![(Tok::Plain, "  filters[0]".to_owned())],
        row("resource_type_filter", &kind_wire(e.kind)),
        row("resource_name_filter", &e.name),
        row("pattern_type_filter", e.pattern.wire()),
        row("principal_filter", &e.principal),
        row("host_filter", &e.host),
        row("operation", &operation_wire(e.operation)),
        row("permission_type", e.permission.wire()),
    ]
}

/// The Create ACL dialog for `input`.
#[must_use]
pub fn create_view(
    version: i16,
    input: AclInput,
    error: Option<(String, String)>,
) -> AclCreateView {
    let e = entry_of(&input);
    AclCreateView {
        call: create_call(version, &e),
        kind_word: kind_label(e.kind).to_lowercase(),
        input,
        error,
        refused: None,
    }
}

/// What is missing from a form, in words, by field.
#[must_use]
pub fn missing(e: &AclEntry) -> Option<(String, String)> {
    if e.name.is_empty() {
        return Some(("name".to_owned(), "Type the resource's name.".to_owned()));
    }
    if e.principal.is_empty() {
        return Some((
            "principal".to_owned(),
            "Type the principal, as Type:name such as User:orders.".to_owned(),
        ));
    }
    None
}

/// Makes the ACL, or says why not in the broker's words.
pub async fn create(b: &Backend, f: &AclFields<'_>) -> AclCreateResult {
    let input = input_of(f);
    let e = entry_of(&input);
    let version = b.version(ApiKey::CreateAcls);
    if let Some(err) = missing(&e) {
        return AclCreateResult::Invalid(Box::new(create_view(version, input, Some(err))));
    }
    let Ok(Response::CreateAcls(r)) = b.call(acls::create(&e)).await else {
        let mut v = create_view(version, input, None);
        v.refused = Some(refusal(-1, Some("The broker did not answer.")));
        return AclCreateResult::Refused(Box::new(v));
    };
    match r.results.first() {
        Some(x) if x.error_code == 0 => AclCreateResult::Created,
        Some(x) => {
            let mut v = create_view(version, input, None);
            v.refused = Some(refusal(
                x.error_code,
                x.error_message.as_ref().map(|m| m.as_str()),
            ));
            AclCreateResult::Refused(Box::new(v))
        }
        None => AclCreateResult::Created,
    }
}

/// The rules the broker decides by now: its default and its super users (the console's own is one, always).
fn rules_of(b: &Backend) -> Rules {
    let known = b.access_facts();
    let mut rules = Rules {
        default_allow: known.as_ref().is_none_or(|k| k.acl_default_allow),
        super_users: known
            .map(|k| k.super_users.into_iter().collect())
            .unwrap_or_default(),
    };
    rules.super_users.insert(CONSOLE_PRINCIPAL.to_owned());
    rules
}

/// The Delete ACL dialog for `target`: what the broker would decide afterwards, or that the ACL is not there.
pub async fn delete_form(b: &Backend, target: AclEntry) -> AclDeleteView {
    let version = b.version(ApiKey::DeleteAcls);
    let call = delete_call(version, &target);
    match entries(b).await {
        Ok(all) if all.contains(&target) => AclDeleteView {
            after: after_delete(&all, &target, &rules_of(b)),
            entry: target,
            call,
            gone: false,
            refused: None,
        },
        Ok(_) => AclDeleteView {
            entry: target,
            after: vec![],
            call,
            gone: true,
            refused: None,
        },
        Err(r) => AclDeleteView {
            entry: target,
            after: vec![],
            call,
            gone: false,
            refused: Some(r),
        },
    }
}

/// Deletes the one ACL.
pub async fn delete(b: &Backend, target: AclEntry) -> AclDeleteResult {
    let Ok(Response::DeleteAcls(r)) = b.call(acls::delete_exact(&target)).await else {
        let mut v = delete_form(b, target).await;
        v.refused = Some(refusal(-1, Some("The broker did not answer.")));
        return AclDeleteResult::Refused(Box::new(v));
    };
    let Some(x) = r.filter_results.first() else {
        return AclDeleteResult::Deleted;
    };
    if x.error_code != 0 {
        let mut v = delete_form(b, target).await;
        v.refused = Some(refusal(
            x.error_code,
            x.error_message.as_ref().map(|m| m.as_str()),
        ));
        return AclDeleteResult::Refused(Box::new(v));
    }
    if x.matching_acls.is_empty() {
        let mut v = delete_form(b, target).await;
        v.gone = true;
        return AclDeleteResult::Gone(Box::new(v));
    }
    AclDeleteResult::Deleted
}

#[cfg(test)]
mod tests;
