//! The Story of a group: the group page's own data, and what the group's ring of notes says. Over a live broker the notes
//! are `ConsoleView::group_events` as the console's principal; over the fixtures they are the larder's (`fixtures_story`).

use kafka_broker::GroupEvents;

use super::group::group;
use crate::backend::Backend;
use crate::model::{GroupLagView, StoryCalm, StoryView};
use crate::story::{calm_of, generations_of, view_of};

/// How many other groups are looked at for the calm one to set beside this.
const MOST_OTHERS: usize = 6;

/// The Story page from what was read (the pure half; the fixtures run it too): `others` are the notes of the other
/// groups, for the contrast card.
#[must_use]
pub fn assemble(
    lag: GroupLagView,
    events: Option<GroupEvents>,
    others: &[(String, GroupEvents)],
) -> StoryView {
    let now_ms = events.as_ref().map_or(lag.head.now_ms, |e| e.now_ms);
    let calm = calmest(others, lag.head.protocol, now_ms);
    view_of(lag.head, events.as_ref(), calm, now_ms)
}

/// The group that has rebalanced least (once or twice, and not now), by name when equal.
fn calmest(
    others: &[(String, GroupEvents)],
    protocol: crate::model::GroupProtocol,
    now_ms: i64,
) -> Option<StoryCalm> {
    let mut found: Vec<StoryCalm> = others
        .iter()
        .filter_map(|(name, e)| calm_of(name, &generations_of(e, protocol, now_ms)))
        .collect();
    found.sort_by(|a, b| a.count.cmp(&b.count).then_with(|| a.group.cmp(&b.group)));
    found.into_iter().next()
}

/// The Story of `id`, or `None` if there is no such group (or the console may not describe it).
pub async fn story(b: &Backend, id: &str) -> Option<StoryView> {
    let lag = group(b, id, false).await?;
    let events = b.group_events(id).await;
    let mut others = Vec::new();
    for other in lag
        .head
        .groups
        .iter()
        .filter(|g| *g != id)
        .take(MOST_OTHERS)
    {
        if let Some(e) = b.group_events(other).await {
            others.push((other.clone(), e));
        }
    }
    Some(assemble(lag, events, &others))
}
