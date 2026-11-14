//! What the Story pages ask of the source: one group's story. Over a live broker it is the group page's data and the
//! group's ring of notes; over the fixtures the same assembly on the larder's notes.

use super::Source;
use crate::gather;
use crate::model::StoryView;

impl Source {
    /// The story of group `id`; `None` if there is no such group or the console may not describe it.
    pub async fn story(&self, id: &str) -> Option<StoryView> {
        match self {
            Self::Live(l) => gather::story::story(&l.backend, id).await,
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => crate::fixtures_story::story(id),
        }
    }
}
