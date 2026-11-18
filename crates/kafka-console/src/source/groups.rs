//! What the Groups pages ask of the source: the list, one group, and the three writes with their dialogs. Over a live broker
//! each is protocol requests as the console's principal; over the fixtures each is the same assembly on the larder's answers.

use super::Source;
use crate::gather::{self, DeleteGroupResult, OffsetDeleteResult, ResetResult};
use crate::model::{
    GroupDeleteView, GroupLagView, GroupStates, GroupsView, OffsetDeleteView, ResetView,
};

impl Source {
    /// The groups whose id contains `filter`, in the state `state`. `hint` names a state for the fixtures only (`none`).
    pub async fn groups(
        &self,
        filter: &str,
        state: &GroupStates,
        hint: Option<&str>,
    ) -> GroupsView {
        match self {
            Self::Live(l) => {
                let _ = hint;
                gather::groups(&l.backend, filter, state).await
            }
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => crate::fixtures_groups::groups(filter, state, hint),
        }
    }

    /// Every group id the broker lists (ListGroups, as the console's principal), by id: what the jump palette offers.
    pub async fn group_names(&self) -> Vec<String> {
        match self {
            Self::Live(l) => gather::group_read::listed(&l.backend)
                .await
                .into_iter()
                .map(|g| g.id)
                .collect(),
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => crate::fixtures_groups::groups("", &GroupStates::default(), None)
                .rows
                .into_iter()
                .map(|r| r.id)
                .collect(),
        }
    }

    /// One group with its lag; `None` if there is no such group or the console may not describe it.
    pub async fn group(&self, id: &str, expiring: bool) -> Option<GroupLagView> {
        match self {
            Self::Live(l) => gather::group(&l.backend, id, expiring).await,
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => crate::fixtures_groups::group(id, expiring),
        }
    }

    /// The Reset dialog for a choice.
    pub async fn reset_form(
        &self,
        id: &str,
        topic: &str,
        to: &str,
        value: &str,
    ) -> Option<ResetView> {
        match self {
            Self::Live(l) => gather::reset_form(&l.backend, id, topic, to, value).await,
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => crate::fixtures_groups::reset(id, topic, to, value),
        }
    }

    /// Applies a reset.
    pub async fn reset_offsets(&self, id: &str, topic: &str, to: &str, value: &str) -> ResetResult {
        match self {
            Self::Live(l) => gather::reset_offsets(&l.backend, id, topic, to, value).await,
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => match crate::fixtures_groups::reset_refused(id) {
                Some(r) if !r.head.members.is_empty() => ResetResult::Refused(Box::new(r)),
                Some(_) => ResetResult::Done(
                    crate::fixtures_groups::reset(id, topic, to, value).map_or(0, |v| v.rows.len()),
                ),
                None => ResetResult::NoGroup,
            },
        }
    }

    /// The Delete-group dialog.
    pub async fn delete_group_form(&self, id: &str) -> Option<GroupDeleteView> {
        match self {
            Self::Live(l) => gather::delete_group_form(&l.backend, id).await,
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => crate::fixtures_groups::delete_form(id),
        }
    }

    /// Deletes a group.
    pub async fn delete_group(&self, id: &str) -> DeleteGroupResult {
        match self {
            Self::Live(l) => gather::delete_group(&l.backend, id).await,
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => match crate::fixtures_groups::delete_refused(id) {
                Some(r) if !r.head.members.is_empty() => DeleteGroupResult::Refused(Box::new(r)),
                Some(_) => DeleteGroupResult::Done,
                None => DeleteGroupResult::NoGroup,
            },
        }
    }

    /// The Delete-offsets dialog with `chosen` topics ticked.
    pub async fn offset_delete_form(
        &self,
        id: &str,
        chosen: &[String],
    ) -> Option<OffsetDeleteView> {
        match self {
            Self::Live(l) => gather::offset_delete_form(&l.backend, id, chosen).await,
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => crate::fixtures_groups::offset_delete_form(id, chosen),
        }
    }

    /// Forgets a group's commits on the topics named.
    pub async fn offset_delete(&self, id: &str, chosen: &[String]) -> OffsetDeleteResult {
        match self {
            Self::Live(l) => gather::offset_delete(&l.backend, id, chosen).await,
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => match crate::fixtures_groups::offset_delete_form(id, chosen) {
                Some(v) if v.chosen.is_empty() => OffsetDeleteResult::Nothing(Box::new(v)),
                Some(v) if !v.head.members.is_empty() => {
                    OffsetDeleteResult::Refused(Box::new(v), crate::groups_words::kafka_refusal(86))
                }
                Some(v) => OffsetDeleteResult::Done(
                    v.topics
                        .iter()
                        .filter(|(t, _)| v.chosen.contains(t))
                        .map(|(_, p)| p.len())
                        .sum(),
                ),
                None => OffsetDeleteResult::NoGroup,
            },
        }
    }
}
