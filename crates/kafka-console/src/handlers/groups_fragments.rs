//! The fragments of the Groups pages, by id: what the stream tells a page to fetch again.

use maud::Markup;

use crate::form::{get, parse};
use crate::model::GroupFilter;
use crate::pages::groups as g;
use crate::state::Shared;

/// The fragment `id`, or `None` for an id that is not one of these.
pub async fn groups_fragment(s: &Shared, id: &str, query: &Option<String>) -> Option<Markup> {
    let f = parse(query.as_deref().unwrap_or(""));
    match id {
        g::GROUPS_LIVE | g::GROUPS_TABLE => {
            let filter = get(&f, "q").unwrap_or("").trim().to_owned();
            let state = GroupFilter::parse(get(&f, "state").unwrap_or(""));
            let v = s.source.groups(&filter, state, None).await;
            Some(if id == g::GROUPS_LIVE {
                g::groups_live_fragment(&v, &s.base)
            } else {
                g::groups_table_fragment(&v, &s.base)
            })
        }
        g::GROUP_STATS => {
            let v = s.source.group(get(&f, "group")?, false).await?;
            Some(g::group_stats_fragment(&v.head, &s.base))
        }
        g::LAG_TABLE => {
            let v = s
                .source
                .group(get(&f, "group")?, get(&f, "expiring") == Some("1"))
                .await?;
            Some(g::lag_table_fragment(&v, &s.base))
        }
        _ => None,
    }
}
