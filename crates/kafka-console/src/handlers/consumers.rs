//! `/topics/{topic}/consumers`: the Consumers tab of a topic. It stays inside the topic: the same head and tabs as every tab
//! of it, and under them the groups that read this topic with their lag on its partitions.

use axum::extract::{Path, State};
use axum::response::Response;
use kafka_wire::ids::TopicName;

use super::html;
use crate::pages::topics as page;
use crate::state::Shared;
use crate::trail::{Place, TopicPage, borrowed};
use crate::ui::shell::Rail;

/// The tab.
pub async fn consumers(State(s): State<Shared>, Path(name): Path<String>) -> Response {
    let found = if TopicName::parse(&name).is_ok() {
        s.source.consumers(&name).await
    } else {
        None
    };
    let Some(view) = found else {
        return super::topics::missing_topic(&s, &name).await;
    };
    let main = page::consumers_page(&view, &s.base);
    let steps = Place::TopicPage(&name, TopicPage::Consumers).steps();
    html(
        &s.document(Rail::Resources, "Consumers", &borrowed(&steps), &[], main)
            .await,
    )
}
