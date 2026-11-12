//! The Lab as a JSON layer calls it: view-models for the rules, the choices of the forms, and the actions, each answered with a typed
//! result in the words of the engine or of Kafka. Everything that needs the broker is here; the rest is `crate::lab`.
//!
//! Nothing here is reachable but through the console's own routes, and every action is a POST the console's same-origin rule has judged.

use crate::backend::Backend;
use crate::lab::Lab;
use crate::lab::act::{self, Choices};
use crate::lab::{compact, views};
use crate::model::lab::{
    LabChoice, LabCompact, LabFields, LabForm, LabHome, LabKind, LabMade, LabOnceDone,
    LabOnceForm, LabOnceKind, LabRuleView,
};
use crate::model::{Cleanup, LogView};
use crate::recorder::Recorder;

/// The most groups and members the expire dialog lists.
const MOST_GROUPS: usize = 24;
const MOST_MEMBERS: usize = 16;

/// What the rule forms choose among: the topics there are and the apps that have spoken.
pub async fn choices(backend: &Backend, recorder: Option<&Recorder>) -> Choices {
    let topics = super::topics(backend, "", false).await;
    let topics = topics
        .rows
        .iter()
        .map(|t| LabChoice {
            value: t.name.clone(),
            label: t.name.clone(),
            desc: Some(format!(
                "{} partition{}{}.",
                t.partitions,
                if t.partitions == 1 { "" } else { "s" },
                if t.cleanup == Cleanup::Delete { "" } else { ", compacted" }
            )),
        })
        .collect();
    let mut clients: Vec<String> = recorder
        .map(|r| r.open_clients().into_iter().map(|(c, _)| c).collect())
        .unwrap_or_default();
    clients.truncate(64);
    Choices { topics, clients }
}

/// The Lab page: the table's rules and the rehearsals kept, and the first compacted topic the cleaner card can open.
pub async fn home(lab: &Lab, backend: &Backend) -> LabHome {
    let mut h = views::home(lab);
    h.compact_target = super::topics(backend, "", false)
        .await
        .rows
        .into_iter()
        .find(|t| t.cleanup != Cleanup::Delete)
        .map(|t| t.name);
    h
}

/// One rule with the requests it touched (from the wire), in force or over.
#[must_use]
pub fn rule(lab: &Lab, recorder: Option<&Recorder>, id: u32) -> Option<LabRuleView> {
    let rule = views::find(lab, id)?;
    let (hits, more_hits) = recorder.map_or((Vec::new(), false), |r| r.lab_hits(id, 8));
    Some(LabRuleView {
        rule,
        now_ms: lab.table().now_ms(),
        hits,
        more_hits,
    })
}

/// The form of a rule for `fields`; with `check`, validation sentences and the preview.
pub async fn rule_form(
    backend: &Backend,
    recorder: Option<&Recorder>,
    kind: LabKind,
    fields: LabFields,
    check: bool,
) -> LabForm {
    act::form_view(kind, fields, choices(backend, recorder).await, check)
}

/// Makes a rule from a form.
pub async fn create(
    lab: &Lab,
    backend: &Backend,
    recorder: Option<&Recorder>,
    kind: LabKind,
    fields: LabFields,
) -> LabMade {
    let c = choices(backend, recorder).await;
    act::make_rule(lab, kind, fields, c)
}

/// Clears a rule now. `false`: it is not in force (it ended already).
pub fn remove(lab: &Lab, id: u32) -> bool {
    lab.table().remove(id)
}

/// Switches a rule on or off. `false`: it is not in force.
pub fn set_enabled(lab: &Lab, id: u32, on: bool) -> bool {
    lab.table().set_enabled(id, on)
}

/// Forgets the rules that are over and the rehearsals kept.
pub fn clear_history(lab: &Lab) {
    lab.clear_history();
}

/// The Drop dialog: the apps with a connection open now.
#[must_use]
pub fn drop_form(recorder: Option<&Recorder>, target: &str, how: &str) -> LabOnceForm {
    let clients = recorder.map(Recorder::open_clients).unwrap_or_default();
    act::drop_form(&clients, target, how)
}

/// Closes an app's connections: once, or for a while.
#[must_use]
pub fn drop_connections(lab: &Lab, recorder: Option<&Recorder>, target: &str, how: &str) -> LabOnceDone {
    let form = drop_form(recorder, target, how);
    let open = recorder
        .map(Recorder::open_clients)
        .unwrap_or_default()
        .iter()
        .find(|(c, _)| c == target)
        .map_or(0, |(_, n)| *n);
    let up_to = recorder.and_then(Recorder::highest_open_conn);
    act::drop_it(lab, form, open, up_to)
}

/// The Expire dialog: every classic or consumer group in memory with its members.
pub async fn expire_form(backend: &Backend, target: &str) -> LabOnceForm {
    let mut f = LabOnceForm {
        kind: LabOnceKind::Expire,
        target: target.to_owned(),
        how: String::new(),
        choices: Vec::new(),
        error: None,
        refused: None,
        note: None,
        nothing: None,
        call: String::new(),
        follows: None,
    };
    for g in super::group_read::listed(backend).await.into_iter().take(MOST_GROUPS) {
        let Some(d) = backend.group_detail(&g.id).await else { continue };
        for m in d.members.iter().take(MOST_MEMBERS) {
            f.choices.push(LabChoice {
                value: act::member_value(&g.id, &m.id),
                label: format!("{} · {}", g.id, m.id),
                desc: Some(format!("{} ({}), {}, generation {}", m.client_id, m.client_host, d.state, d.generation)),
            });
        }
    }
    if f.choices.is_empty() {
        f.nothing = Some("No group has a member right now.".to_owned());
    }
    if let Some((g, m)) = act::split_member(target) {
        f.call = format!("doze:ExpireMember group={g} member={m}");
    }
    f
}

/// Runs a member's session out.
pub async fn expire(lab: &Lab, backend: &Backend, target: &str) -> LabOnceDone {
    act::expire(lab, backend, expire_form(backend, target).await).await
}

/// The Rebalance dialog: the groups in memory with members.
pub async fn rebalance_form(backend: &Backend, target: &str) -> LabOnceForm {
    let mut f = expire_form(backend, "").await;
    f.kind = LabOnceKind::Rebalance;
    f.target = target.to_owned();
    f.choices.clear();
    for g in super::group_read::listed(backend).await.into_iter().take(MOST_GROUPS) {
        let Some(d) = backend.group_detail(&g.id).await else { continue };
        if !d.members.is_empty() {
            f.choices.push(LabChoice {
                value: g.id.clone(),
                label: g.id.clone(),
                desc: Some(format!("{}, {} member{}, generation {}", d.state, d.members.len(), if d.members.len() == 1 { "" } else { "s" }, d.generation)),
            });
        }
    }
    f.nothing = f.choices.is_empty().then(|| "No group has a member right now.".to_owned());
    f.call = format!("doze:ForceRebalance group={target}");
    f
}

/// Forces a rebalance.
pub async fn rebalance(lab: &Lab, backend: &Backend, target: &str) -> LabOnceDone {
    act::rebalance(lab, backend, rebalance_form(backend, target).await).await
}

/// The Compact now dialog of a partition; `None` if there is no such topic.
pub async fn compact_form(backend: &Backend, lab_on: bool, topic: &str, partition: i32) -> Option<LabCompact> {
    let log: LogView = super::log_view(backend, topic, partition).await?;
    Some(compact::dialog(&log, lab_on))
}

/// Runs the cleaner's round now. `Ok`: it ran. `Err`: the dialog again, with what the broker said.
///
/// # Errors
/// The partition is not compacted, nothing is due, or the round failed: the dialog with its words.
pub async fn compact(lab: &Lab, backend: &Backend, topic: &str, partition: i32) -> Result<(), Box<LabCompact>> {
    let Some(v) = compact_form(backend, true, topic, partition).await else {
        return Err(Box::new(LabCompact {
            topic: topic.to_owned(),
            partition,
            due: None,
            rewrite: Vec::new(),
            checkpoint: (0, 0),
            blocked: None,
            refused: Some("There is no such topic or partition.".to_owned()),
            off: false,
        }));
    };
    if let Some(b) = &v.blocked {
        let mut v = v.clone();
        v.refused = Some(b.clone());
        return Err(Box::new(v));
    }
    compact::run(lab, backend, v).await
}
