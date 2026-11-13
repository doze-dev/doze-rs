//! Ready-made rehearsals: each a question about your app and the dialog filled in to answer it, with the broker's own topics,
//! apps and groups. Pure: [`recipes`] is a function of what the broker has ([`Seen`]); `gather::lab::guide` reads it.

use crate::model::lab::LabFields;
use crate::model::lab_guide::LabRecipe;

/// What the broker has that a recipe can name.
#[derive(Debug, Clone, Default)]
pub struct Seen {
    /// The topics, internal ones left out.
    pub topics: Vec<String>,
    /// The busiest topic written to on the wire, and the app writing to it most.
    pub producing: Option<(String, String)>,
    /// The busiest topic read from on the wire.
    pub fetching: Option<String>,
    /// The apps with a connection open now.
    pub clients: Vec<String>,
    /// A group with members.
    pub group: Option<String>,
    /// A member of it, as the Expire dialog's value (`group/member`).
    pub member: Option<String>,
    pub compacted: Vec<String>,
}

const NO_TOPIC: &str = "a topic";
const NO_APP: &str = "an app";
const NO_GROUP: &str = "a group";

fn rule(api: &str, topic: &str, client: &str, share: &str, ends: &str) -> LabFields {
    LabFields {
        api: api.to_owned(),
        topic: topic.to_owned(),
        client: client.to_owned(),
        share: share.to_owned(),
        ends: ends.to_owned(),
        ..LabFields::default()
    }
}

#[allow(clippy::too_many_arguments)]
fn recipe(
    id: &str,
    tab: &str,
    title: String,
    question: &str,
    fields: Option<LabFields>,
    target: Option<String>,
    how: Option<&str>,
    needs: Option<&str>,
) -> LabRecipe {
    LabRecipe {
        id: id.to_owned(),
        tab: tab.to_owned(),
        title,
        question: question.to_owned(),
        fields,
        target,
        how: how.map(str::to_owned),
        needs: needs.map(str::to_owned),
    }
}

/// The recipes, in the order a newcomer would try them. One the broker cannot serve now says what it needs.
#[must_use]
pub fn recipes(seen: &Seen) -> Vec<LabRecipe> {
    let topic = seen
        .producing
        .as_ref()
        .map(|(t, _)| t.clone())
        .or_else(|| seen.topics.first().cloned());
    let read = seen.fetching.clone().or_else(|| topic.clone());
    let app = seen
        .producing
        .as_ref()
        .map(|(_, c)| c.clone())
        .filter(|c| seen.clients.contains(c))
        .or_else(|| seen.clients.first().cloned());
    let need_topic = topic.is_none().then_some("Needs a topic.");
    let need_app = app
        .is_none()
        .then_some("Needs an app with a connection open.");
    let need_group = seen
        .group
        .is_none()
        .then_some("Needs a group with members.");
    let t = topic.clone().unwrap_or_default();
    let tn = topic.as_deref().unwrap_or(NO_TOPIC);
    let r = read.clone().unwrap_or_default();
    let a = app.clone().unwrap_or_default();
    let an = app.as_deref().unwrap_or(NO_APP);
    let gn = seen.group.as_deref().unwrap_or(NO_GROUP);
    let mut slow_fetch = rule("Fetch", &r, "", "every", "5");
    slow_fetch.delay = "2000".to_owned();
    slow_fetch.jitter = "1000".to_owned();
    let mut slow_produce = rule("Produce", &t, "", "every", "10");
    slow_produce.delay = "800".to_owned();
    let mut leader = rule("Produce", &t, "", "5", "5");
    leader.code = "6".to_owned();
    let mut timeout = rule("Produce", &t, &a, "10", "5");
    timeout.code = "7".to_owned();
    vec![
        recipe(
            "slow-produce",
            "slow",
            format!("Slow every Produce to {tn} by 800 ms"),
            "Does your producer keep up, or do sends start to time out and pile up?",
            Some(slow_produce),
            None,
            None,
            need_topic,
        ),
        recipe(
            "slow-fetch",
            "slow",
            format!(
                "Slow Fetch from {} by 2 s, give or take 1 s",
                read.as_deref().unwrap_or(NO_TOPIC)
            ),
            "Does your consumer's lag grow and then drain, without the group rebalancing?",
            Some(slow_fetch),
            None,
            None,
            need_topic,
        ),
        recipe(
            "fail-leader",
            "fail",
            format!("Fail 1 in 5 Produce to {tn} with NOT_LEADER_OR_FOLLOWER"),
            "A leader moves in a rolling restart: does your producer retry without duplicates?",
            Some(leader),
            None,
            None,
            need_topic,
        ),
        recipe(
            "fail-timeout",
            "fail",
            format!("Time out 1 in 10 Produce from {an} to {tn}"),
            "Does a request that seems lost get retried, and each record written once?",
            Some(timeout),
            None,
            None,
            need_topic.or(need_app),
        ),
        recipe(
            "drop-once",
            "drop",
            format!("Drop {an}'s connections once"),
            "Does it reconnect on its own and carry on where it was?",
            None,
            app.clone(),
            Some("once"),
            need_app,
        ),
        recipe(
            "drop-flap",
            "drop",
            format!("Keep dropping {an} for 1 minute"),
            "A flapping network: does it back off between tries, or hammer the broker?",
            None,
            app,
            Some("1"),
            need_app,
        ),
        recipe(
            "expire-member",
            "expire",
            format!("Expire one member of {gn}"),
            "Does it notice, join again and resume without processing anything twice?",
            None,
            seen.member.clone(),
            None,
            need_group.or(seen
                .member
                .is_none()
                .then_some("Needs a group with members.")),
        ),
        recipe(
            "rebalance-group",
            "rebalance",
            format!("Rebalance {gn}, as a deploy would"),
            "Do your consumers commit before giving partitions up, and pick up cleanly after?",
            None,
            seen.group.clone(),
            None,
            need_group,
        ),
        recipe(
            "compact-now",
            "compact",
            format!(
                "Compact {} now",
                seen.compacted
                    .first()
                    .map_or("a compacted topic", String::as_str)
            ),
            "What does a compacted topic keep of each key, and what do readers see?",
            None,
            seen.compacted.first().cloned(),
            None,
            seen.compacted
                .is_empty()
                .then_some("Needs a topic with cleanup.policy=compact."),
        ),
    ]
}

#[cfg(test)]
mod tests;
