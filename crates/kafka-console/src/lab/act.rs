//! What the Lab does when it is asked: makes a rule from a form, or carries out a rehearsal that happens once. Each answers with the
//! form to show again (with the sentence for each field to fix) or that it is done; each rehearsal leaves a line in the Ended list.

use kafka_broker::LabOutcome;
use kafka_server::fault::{Effect, Matcher, Pattern, Share, Spec};

use super::form::{self, field_of_refusal, preview, spec_of};
use super::words;
use crate::backend::Backend;
use crate::lab::Lab;
use crate::model::lab::{
    LabChoice, LabFields, LabForm, LabKind, LabMade, LabOnceDone, LabOnceForm, LabOnceKind,
};

/// What a form needs from the broker to be drawn: the topics and the apps there are to choose from.
#[derive(Debug, Clone, Default)]
pub struct Choices {
    pub topics: Vec<LabChoice>,
    pub clients: Vec<String>,
}

/// The form of a rule for `fields`; with `check`, or when the fields make a rule, the preview of what it would do.
#[must_use]
pub fn form_view(kind: LabKind, fields: LabFields, choices: Choices, check: bool) -> LabForm {
    let fields = form::filled(kind, fields);
    let made = spec_of(kind, &fields);
    let (errors, preview) = match &made {
        Ok(spec) => (Vec::new(), Some(preview(spec))),
        Err(e) if check => (e.clone(), None),
        Err(_) => (Vec::new(), None),
    };
    let topic_note = choices
        .topics
        .iter()
        .find(|t| t.value == fields.topic)
        .and_then(|t| t.desc.clone());
    LabForm {
        kind,
        fields,
        errors,
        topics: choices.topics,
        clients: choices.clients,
        topic_note,
        preview,
        refused: None,
    }
}

/// Makes the rule a form asks for, or answers with the form and what is wrong.
#[must_use]
pub fn make_rule(lab: &Lab, kind: LabKind, fields: LabFields, choices: Choices) -> LabMade {
    let fields = form::filled(kind, fields);
    match spec_of(kind, &fields) {
        Ok(spec) => match lab.table().add(spec.clone()) {
            Ok(id) => LabMade::Made(id),
            Err(e) => {
                let mut v = form_view(kind, fields, choices, true);
                match field_of_refusal(e) {
                    Some(f) => v.errors.push((f.to_owned(), words::refused_words(e))),
                    None => v.refused = Some(words::refused_words(e)),
                }
                v.preview = Some(preview(&spec));
                LabMade::Invalid(Box::new(v))
            }
        },
        Err(errors) => {
            let mut v = form_view(kind, fields, choices, true);
            v.errors = errors;
            LabMade::Invalid(Box::new(v))
        }
    }
}

fn once_form(kind: LabOnceKind, target: &str, how: &str) -> LabOnceForm {
    LabOnceForm {
        kind,
        target: target.to_owned(),
        how: how.to_owned(),
        choices: Vec::new(),
        error: None,
        refused: None,
        note: None,
        nothing: None,
        call: String::new(),
        follows: None,
    }
}

/// How long a rule that keeps dropping lasts, by the form's word.
#[must_use]
pub fn drop_ttl(how: &str) -> Option<u64> {
    match how {
        "once" => Some(60),
        "1" => Some(60),
        "5" => Some(300),
        _ => None,
    }
}

/// The Drop dialog: the apps with a connection open now.
#[must_use]
pub fn drop_form(clients: &[(String, usize)], target: &str, how: &str) -> LabOnceForm {
    let how = if drop_ttl(how).is_some() { how } else { "once" };
    let mut f = once_form(LabOnceKind::Drop, target, how);
    f.choices = clients
        .iter()
        .map(|(c, n)| LabChoice {
            value: c.clone(),
            label: c.clone(),
            desc: Some(format!(
                "{n} connection{} open now",
                if *n == 1 { "" } else { "s" }
            )),
        })
        .collect();
    if clients.is_empty() {
        f.nothing = Some("No app has a connection open that has said who it is.".to_owned());
    }
    let who = if target.is_empty() {
        "<client>"
    } else {
        target
    };
    f.call = if how == "once" {
        format!("doze:DropConnection client={who} once")
    } else {
        format!("doze:DropConnection client={who} for={how}m")
    };
    f
}

/// Drops an app's connections, or keeps dropping them for a while: a rule that ends by itself. `open` is how many connections the
/// app has now, and `up_to` the highest connection number there is.
#[must_use]
pub fn drop_it(lab: &Lab, form: LabOnceForm, open: usize, up_to: Option<u64>) -> LabOnceDone {
    let mut f = form;
    let Some(client) = Pattern::new(&f.target).filter(|_| !f.target.contains(['*', '?'])) else {
        f.error = Some("Choose the app whose connections to close.".to_owned());
        return LabOnceDone::Invalid(Box::new(f));
    };
    let once = f.how == "once";
    if once && (open == 0 || up_to.is_none()) {
        f.error = Some(format!("{} has no connection open now.", f.target));
        return LabOnceDone::Invalid(Box::new(f));
    }
    let ttl = drop_ttl(&f.how).unwrap_or(60);
    let spec = Spec {
        name: String::new(),
        matcher: Matcher {
            client: Some(client),
            up_to_conn: once.then_some(up_to).flatten(),
            ..Matcher::default()
        },
        effect: Effect::Drop,
        share: Share::Every,
        ttl: std::time::Duration::from_secs(ttl),
        max_hits: once.then(|| u32::try_from(open).unwrap_or(u32::MAX)),
    };
    let spec = Spec {
        name: words::default_name(&spec),
        ..spec
    };
    match lab.table().add(spec) {
        Ok(_) => LabOnceDone::Done,
        Err(e) => {
            f.refused = Some(words::refused_words(e));
            LabOnceDone::Invalid(Box::new(f))
        }
    }
}

/// The word for what the broker answered a forced step, and whether it was done.
fn outcome_words(o: LabOutcome) -> (bool, &'static str) {
    match o {
        LabOutcome::Done => (true, "done"),
        LabOutcome::Denied => (false, "The console may not do that on this broker."),
        LabOutcome::NoSuchGroup => (
            false,
            "The broker has no such group in memory: it may have been deleted, or never had a member since the broker began.",
        ),
        LabOutcome::NoSuchMember => (false, "The group has no such member any more."),
        LabOutcome::Empty => (
            false,
            "The group has no member, so there is nothing to rebalance.",
        ),
        LabOutcome::AlreadyRebalancing => (
            false,
            "The group is rebalancing already; wait for it to settle, then force another.",
        ),
    }
}

/// Splits a `group/member` choice made by [`member_value`].
#[must_use]
pub fn split_member(value: &str) -> Option<(String, String)> {
    let (g, m) = value.split_once('/')?;
    Some((crate::form::decode(g), crate::form::decode(m)))
}

/// The value of a choice that names a group and one of its members.
#[must_use]
pub fn member_value(group: &str, member: &str) -> String {
    format!(
        "{}/{}",
        crate::base::segment(group),
        crate::base::segment(member)
    )
}

/// Expires a member's session; the answer is the dialog again, with the broker's words, or that it is done.
pub async fn expire(lab: &Lab, backend: &Backend, mut form: LabOnceForm) -> LabOnceDone {
    let Some((group, member)) = split_member(&form.target) else {
        form.error = Some("Choose the member whose session to expire.".to_owned());
        return LabOnceDone::Invalid(Box::new(form));
    };
    let outcome = backend.lab_expire(&group, &member).await;
    let (ok, words) = outcome_words(outcome);
    let shown = |s: &str| s.chars().take(80).collect::<String>();
    lab.note(
        LabOnceKind::Expire,
        if ok {
            format!(
                "Ran out the session of {} in {}.",
                shown(&member),
                shown(&group)
            )
        } else {
            format!(
                "Could not expire {} in {}: {words}",
                shown(&member),
                shown(&group)
            )
        },
        ok,
    );
    if ok {
        LabOnceDone::Done
    } else {
        form.refused = Some(words.to_owned());
        LabOnceDone::Invalid(Box::new(form))
    }
}

/// Forces a rebalance of a group.
pub async fn rebalance(lab: &Lab, backend: &Backend, mut form: LabOnceForm) -> LabOnceDone {
    if form.target.is_empty() {
        form.error = Some("Choose the group to rebalance.".to_owned());
        return LabOnceDone::Invalid(Box::new(form));
    }
    let group = form.target.clone();
    let outcome = backend.lab_rebalance(&group).await;
    let (ok, words) = outcome_words(outcome);
    let shown: String = group.chars().take(80).collect();
    lab.note(
        LabOnceKind::Rebalance,
        if ok {
            format!("Forced a rebalance of {shown}.")
        } else {
            format!("Could not rebalance {shown}: {words}")
        },
        ok,
    );
    if ok {
        LabOnceDone::Done
    } else {
        form.refused = Some(words.to_owned());
        LabOnceDone::Invalid(Box::new(form))
    }
}

#[cfg(test)]
mod tests;
